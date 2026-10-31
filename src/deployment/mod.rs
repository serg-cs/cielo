use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    fs,
    future::Future,
    path::{Component, Path},
};

use anyhow::{Context, Result, anyhow, bail};
use aws_config::{BehaviorVersion, SdkConfig, meta::region::RegionProviderChain};
use aws_sdk_s3::{
    Client,
    config::{Builder as S3ConfigBuilder, Region},
    primitives::ByteStream,
    types::{Delete, ObjectIdentifier},
};
use md5::{Digest, Md5};
use tokio::task::JoinSet;
use tracing::{info, warn};

use crate::cli::DeployTargetArgs;

#[cfg(test)]
mod tests;

const DEFAULT_REGION: &str = "us-east-1";
const APP_ENTRY_POINT: &str = "index.html";
const APP_SERVICE_WORKER: &str = "sw.js";
const DATA_ENTRY_POINT: &str = "v4/places.json";
/// Local build marker that is never uploaded.
const DATA_MARKER: &str = ".cielo-data";
const CONTENT_HASH_LENGTH: usize = 16;
const LEGACY_CONTENT_HASH_LENGTH: usize = 64;
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const REVALIDATE_CACHE_CONTROL: &str = "no-cache, must-revalidate";
const DATA_CACHE_CONTROL: &str = "no-cache";
const DELETE_BATCH_SIZE: usize = 1000;

/// Select validation and ordering rules for a deployment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeploymentKind {
    App,
    Data,
}

impl DeploymentKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Data => "data",
        }
    }
}

/// What one deployment changed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DeploymentSummary {
    pub(crate) uploaded: usize,
    pub(crate) skipped: usize,
    pub(crate) deleted: usize,
    pub(crate) bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeploymentFile {
    key: String,
    content_type: String,
    cache_control: Option<&'static str>,
    body: Vec<u8>,
    /// MD5 of `body` as lowercase hex, compared with the remote `ETag`.
    md5: String,
}

impl DeploymentFile {
    fn is_immutable(&self) -> bool {
        self.cache_control == Some(IMMUTABLE_CACHE_CONTROL)
    }
}

pub(crate) async fn deploy_directory(
    args: &DeployTargetArgs,
    kind: DeploymentKind,
) -> Result<DeploymentSummary> {
    validate_connection_options(args)?;
    if args.prune && kind == DeploymentKind::App {
        bail!("--prune is only supported for data: old app assets may still be in use");
    }

    // Validate and order every local file before changing remote state.
    let files = prepare_deployment(&args.input, kind)?;

    // Reuse the standard AWS provider chain while allowing S3-compatible endpoints.
    let client = create_client(args).await;
    let prefixes = owned_prefixes(&files, kind);
    let remote = match list_remote(&client, &args.bucket, &prefixes).await {
        Ok(remote) => Some(remote),
        Err(error) if !args.prune => {
            warn!(
                error = format!("{error:#}"),
                "could not list the bucket; uploading every file"
            );
            None
        }
        Err(error) => return Err(error.context("failed to list the bucket for --prune")),
    };

    let plan = plan_deployment(files, remote.as_ref(), args.prune);
    let summary = DeploymentSummary {
        uploaded: plan.uploads.len(),
        skipped: plan.skipped,
        deleted: plan.deletions.len(),
        bytes: plan
            .uploads
            .iter()
            .map(|file| usize_to_u64(file.body.len()))
            .sum(),
    };
    if args.dry_run {
        for file in &plan.uploads {
            info!(key = %file.key, bytes = file.body.len(), "would upload");
        }
        for key in &plan.deletions {
            info!(%key, "would delete");
        }
        return Ok(summary);
    }

    upload_files(
        &client,
        &args.bucket,
        &plan.uploads,
        usize::from(args.concurrency),
    )
    .await?;
    delete_objects(&client, &args.bucket, &plan.deletions).await?;
    Ok(summary)
}

struct DeploymentPlan {
    uploads: Vec<DeploymentFile>,
    skipped: usize,
    deletions: Vec<String>,
}

/// Decide which files to upload and which remote objects to delete.
///
/// A file is skipped when the remote object has the same MD5 `ETag`, or when
/// it is an immutable content-hashed asset that already exists.
fn plan_deployment(
    files: Vec<DeploymentFile>,
    remote: Option<&HashMap<String, String>>,
    prune: bool,
) -> DeploymentPlan {
    let local_keys = files
        .iter()
        .map(|file| file.key.clone())
        .collect::<BTreeSet<_>>();
    let mut uploads = Vec::new();
    let mut skipped = 0;
    for file in files {
        let unchanged = remote
            .and_then(|remote| remote.get(&file.key))
            .is_some_and(|etag| file.is_immutable() || normalize_etag(etag) == file.md5);
        if unchanged {
            skipped += 1;
        } else {
            uploads.push(file);
        }
    }
    let deletions = match remote {
        Some(remote) if prune => {
            let mut keys = remote
                .keys()
                .filter(|key| !local_keys.contains(*key))
                .cloned()
                .collect::<Vec<_>>();
            keys.sort();
            keys
        }
        _ => Vec::new(),
    };
    DeploymentPlan {
        uploads,
        skipped,
        deletions,
    }
}

fn normalize_etag(etag: &str) -> String {
    etag.trim().trim_matches('"').to_ascii_lowercase()
}

/// Remote prefixes this deployment is responsible for.
///
/// Data deployments own the top-level entries under `v4/` they contain (for
/// example `v4/forecast/` and `v4/places.json`), so a deployment of one product
/// family never touches another's files. App deployments own the whole bucket.
fn owned_prefixes(files: &[DeploymentFile], kind: DeploymentKind) -> Vec<String> {
    if kind == DeploymentKind::App {
        return vec![String::new()];
    }
    let mut prefixes = BTreeSet::new();
    for file in files {
        let mut parts = file.key.splitn(3, '/');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(version), Some(directory), Some(_)) => {
                prefixes.insert(format!("{version}/{directory}/"));
            }
            _ => {
                prefixes.insert(file.key.clone());
            }
        }
    }
    prefixes.into_iter().collect()
}

async fn list_remote(
    client: &Client,
    bucket: &str,
    prefixes: &[String],
) -> Result<HashMap<String, String>> {
    let mut objects = HashMap::new();
    for prefix in prefixes {
        let mut token = None;
        loop {
            let response = client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(prefix)
                .set_continuation_token(token)
                .send()
                .await
                .map_err(|error| {
                    anyhow!(
                        "failed to list objects under '{prefix}': {}",
                        aws_sdk_s3::error::DisplayErrorContext(&error)
                    )
                })?;
            for object in response.contents() {
                let Some(key) = object.key() else {
                    continue;
                };
                // A file prefix such as `v4/places.json` also matches longer keys.
                if !prefix.ends_with('/') && !prefix.is_empty() && key != prefix {
                    continue;
                }
                objects.insert(
                    key.to_owned(),
                    object.e_tag().unwrap_or_default().to_owned(),
                );
            }
            token = response.next_continuation_token().map(str::to_owned);
            if token.is_none() {
                break;
            }
        }
    }
    Ok(objects)
}

async fn delete_objects(client: &Client, bucket: &str, keys: &[String]) -> Result<()> {
    for batch in keys.chunks(DELETE_BATCH_SIZE) {
        let objects = batch
            .iter()
            .map(|key| ObjectIdentifier::builder().key(key).build())
            .collect::<Result<Vec<_>, _>>()
            .context("invalid object key")?;
        let delete = Delete::builder()
            .set_objects(Some(objects))
            .quiet(true)
            .build()
            .context("invalid delete request")?;
        let response = client
            .delete_objects()
            .bucket(bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|error| {
                anyhow!(
                    "failed to delete stale objects: {}",
                    aws_sdk_s3::error::DisplayErrorContext(&error)
                )
            })?;
        if let Some(error) = response.errors().first() {
            bail!(
                "failed to delete {}: {}",
                error.key().unwrap_or("an object"),
                error.message().unwrap_or("unknown error")
            );
        }
    }
    Ok(())
}

fn validate_connection_options(args: &DeployTargetArgs) -> Result<()> {
    if args.bucket.trim().is_empty() {
        bail!("bucket name must not be empty");
    }
    if args.concurrency == 0 {
        bail!("deployment concurrency must be greater than zero");
    }
    if args
        .endpoint
        .as_deref()
        .is_some_and(|endpoint| endpoint.trim().is_empty())
    {
        bail!("endpoint must not be empty");
    }
    if args
        .region
        .as_deref()
        .is_some_and(|region| region.trim().is_empty())
    {
        bail!("region must not be empty");
    }

    Ok(())
}

async fn create_client(args: &DeployTargetArgs) -> Client {
    let explicit_region = args.region.clone().map(Region::new);
    let region = RegionProviderChain::first_try(explicit_region)
        .or_default_provider()
        .or_else(Region::new(DEFAULT_REGION));
    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(region)
        .load()
        .await;

    Client::from_conf(create_service_config(&shared_config, args))
}

fn create_service_config(shared_config: &SdkConfig, args: &DeployTargetArgs) -> aws_sdk_s3::Config {
    let mut service_config = S3ConfigBuilder::from(shared_config);
    if let Some(endpoint) = &args.endpoint {
        service_config = service_config.endpoint_url(endpoint);
    }

    service_config.force_path_style(args.path_style).build()
}

fn prepare_deployment(input: &Path, kind: DeploymentKind) -> Result<Vec<DeploymentFile>> {
    let input_metadata = fs::symlink_metadata(input)
        .with_context(|| format!("failed to inspect input directory {}", input.display()))?;
    if input_metadata.file_type().is_symlink() {
        bail!(
            "input directory must not be a symbolic link: {}",
            input.display()
        );
    }
    if !input_metadata.is_dir() {
        bail!("input path is not a directory: {}", input.display());
    }

    // Walk without following links so object keys cannot escape the selected tree.
    let mut directories = vec![input.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("failed to read input directory {}", directory.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "failed to read an entry in input directory {}",
                    directory.display()
                )
            })?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to inspect input path {}", path.display()))?;
            if file_type.is_symlink() {
                bail!("input contains a symbolic link: {}", path.display());
            }
            if file_type.is_dir() {
                directories.push(path);
                continue;
            }
            if !file_type.is_file() {
                bail!(
                    "input contains an unsupported file type: {}",
                    path.display()
                );
            }

            let relative = path.strip_prefix(input).with_context(|| {
                format!(
                    "input file {} is outside directory {}",
                    path.display(),
                    input.display()
                )
            })?;
            let key = object_key(relative)?;
            if kind == DeploymentKind::Data && key == DATA_MARKER {
                continue;
            }
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read input file {}", path.display()))?;
            let content_type = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .essence_str()
                .to_owned();
            files.push(DeploymentFile {
                cache_control: cache_control(&key, kind),
                md5: md5_hex(&bytes),
                key,
                content_type,
                body: bytes,
            });
        }
    }

    if files.is_empty() {
        bail!("input directory contains no files: {}", input.display());
    }

    // Keep deployment order stable and publish consumer entry points last.
    files.sort_unstable_by(|left, right| left.key.cmp(&right.key));
    match kind {
        DeploymentKind::App => {
            if !files.iter().any(|file| file.key == APP_ENTRY_POINT) {
                bail!(
                    "app input directory does not contain root {APP_ENTRY_POINT}: {}",
                    input.display()
                );
            }
            move_to_end(&mut files, APP_SERVICE_WORKER);
            move_to_end(&mut files, APP_ENTRY_POINT);
        }
        DeploymentKind::Data => move_to_end(&mut files, DATA_ENTRY_POINT),
    }

    Ok(files)
}

fn move_to_end(files: &mut Vec<DeploymentFile>, key: &str) {
    if let Some(index) = files.iter().position(|file| file.key == key) {
        let file = files.remove(index);
        files.push(file);
    }
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn object_key(relative: &Path) -> Result<String> {
    let mut components = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!(
                "input file has an unsupported relative path: {}",
                relative.display()
            );
        };
        let component = component.to_str().with_context(|| {
            format!("input file path is not valid UTF-8: {}", relative.display())
        })?;
        components.push(component);
    }
    if components.is_empty() {
        bail!("input file has an empty object key");
    }

    Ok(components.join("/"))
}

fn cache_control(key: &str, kind: DeploymentKind) -> Option<&'static str> {
    if is_content_hashed_key(key) {
        return Some(IMMUTABLE_CACHE_CONTROL);
    }
    match kind {
        DeploymentKind::App if key == APP_ENTRY_POINT || key == APP_SERVICE_WORKER => {
            Some(REVALIDATE_CACHE_CONTROL)
        }
        DeploymentKind::App => None,
        DeploymentKind::Data => Some(DATA_CACHE_CONTROL),
    }
}

fn is_content_hashed_key(key: &str) -> bool {
    let file_name = key.rsplit('/').next().unwrap_or(key);
    let Some((stem_and_hash, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    let Some((stem, hash)) = stem_and_hash.rsplit_once('.') else {
        return false;
    };

    !stem.is_empty()
        && !extension.is_empty()
        && matches!(hash.len(), CONTENT_HASH_LENGTH | LEGACY_CONTENT_HASH_LENGTH)
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

async fn upload_files(
    client: &Client,
    bucket: &str,
    files: &[DeploymentFile],
    concurrency: usize,
) -> Result<()> {
    if concurrency == 0 {
        bail!("deployment concurrency must be greater than zero");
    }
    // Entry points go last, one at a time, after every other file succeeded.
    let entry_points = files
        .iter()
        .rev()
        .take_while(|file| {
            matches!(
                file.key.as_str(),
                APP_ENTRY_POINT | APP_SERVICE_WORKER | DATA_ENTRY_POINT
            )
        })
        .count();
    let (assets, entry_points) = files.split_at(files.len() - entry_points);

    let upload_client = client.clone();
    let upload_bucket = bucket.to_owned();
    run_bounded_uploads(assets.iter().cloned(), concurrency, move |file| {
        let client = upload_client.clone();
        let bucket = upload_bucket.clone();
        async move { upload_file(&client, &bucket, &file).await }
    })
    .await?;
    for file in entry_points {
        upload_file(client, bucket, file).await?;
    }
    Ok(())
}

async fn run_bounded_uploads<T, I, F, Fut>(items: I, concurrency: usize, upload: F) -> Result<()>
where
    T: Send + 'static,
    I: IntoIterator<Item = T>,
    F: Fn(T) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    if concurrency == 0 {
        bail!("deployment concurrency must be greater than zero");
    }

    // Fill the initial window, then replenish it as uploads finish.
    let mut items = items.into_iter();
    let mut uploads = JoinSet::new();
    for item in items.by_ref().take(concurrency) {
        uploads.spawn(upload(item));
    }

    let mut first_error = None;
    while let Some(joined) = uploads.join_next().await {
        let result = match joined {
            Ok(result) => result,
            Err(_) => Err(anyhow!("file upload task failed")),
        };
        if let Err(error) = result {
            if first_error.is_none() {
                first_error = Some(error);
            }
            continue;
        }
        if first_error.is_none()
            && let Some(item) = items.next()
        {
            uploads.spawn(upload(item));
        }
    }

    first_error.map_or(Ok(()), Err)
}

async fn upload_file(client: &Client, bucket: &str, file: &DeploymentFile) -> Result<()> {
    let mut upload = client
        .put_object()
        .bucket(bucket)
        .key(&file.key)
        .content_type(&file.content_type)
        .body(ByteStream::from(file.body.clone()));
    if let Some(cache_control) = file.cache_control {
        upload = upload.cache_control(cache_control);
    }
    upload
        .send()
        .await
        .map_err(|_| anyhow!("failed to upload {}", file.key))?;

    Ok(())
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
