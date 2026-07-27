use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use aws_config::{BehaviorVersion, SdkConfig, meta::region::RegionProviderChain};
use aws_sdk_s3::{
    Client,
    config::{Builder as S3ConfigBuilder, Region},
    error::DisplayErrorContext,
    primitives::ByteStream,
};

use crate::cli::DeployTargetArgs;

#[cfg(test)]
mod tests;

const DEFAULT_REGION: &str = "us-east-1";
const APP_ENTRY_POINT: &str = "index.html";
const DATA_ENTRY_POINT: &str = "catalog.json";
const CONTENT_HASH_LENGTH: usize = 64;
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

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

/// Report the files successfully uploaded by one deployment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DeploymentSummary {
    pub(crate) files: usize,
    pub(crate) bytes: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct DeploymentFile {
    source: PathBuf,
    key: String,
    content_type: String,
    bytes: u64,

    cache_control: Option<&'static str>,
}

pub(crate) async fn deploy_directory(
    args: &DeployTargetArgs,
    kind: DeploymentKind,
) -> Result<DeploymentSummary> {
    validate_connection_options(args)?;

    // Validate and order every local file before changing remote state.
    let files = prepare_deployment(&args.input, kind)?;
    let bytes = files.iter().try_fold(0_u64, |total, file| {
        total
            .checked_add(file.bytes)
            .context("deployment size exceeds the supported range")
    })?;

    // Reuse the standard AWS provider chain while allowing S3-compatible endpoints.
    let client = create_client(args).await;
    upload_files(&client, &args.bucket, &files).await?;

    Ok(DeploymentSummary {
        files: files.len(),
        bytes,
    })
}

fn validate_connection_options(args: &DeployTargetArgs) -> Result<()> {
    if args.bucket.trim().is_empty() {
        bail!("bucket name must not be empty");
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
            let metadata = entry
                .metadata()
                .with_context(|| format!("failed to inspect input file {}", path.display()))?;
            let key = object_key(relative)?;
            let content_type = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .essence_str()
                .to_owned();
            let cache_control = cache_control(&key);
            files.push(DeploymentFile {
                source: path,
                key,
                content_type,
                bytes: metadata.len(),

                cache_control,
            });
        }
    }

    if files.is_empty() {
        bail!("input directory contains no files: {}", input.display());
    }

    // Keep deployment order stable and publish each consumer entry point only at the end.
    files.sort_unstable_by(|left, right| left.key.cmp(&right.key));
    let entry_point = match kind {
        DeploymentKind::App => APP_ENTRY_POINT,
        DeploymentKind::Data => DATA_ENTRY_POINT,
    };
    let index = files
        .iter()
        .position(|file| file.key == entry_point)
        .with_context(|| {
            format!(
                "{} input directory does not contain root {entry_point}: {}",
                kind.as_str(),
                input.display()
            )
        })?;
    let entry_point = files.remove(index);
    files.push(entry_point);

    Ok(files)
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

fn cache_control(key: &str) -> Option<&'static str> {
    is_content_hashed_key(key).then_some(IMMUTABLE_CACHE_CONTROL)
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
        && hash.len() == CONTENT_HASH_LENGTH
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

async fn upload_files(client: &Client, bucket: &str, files: &[DeploymentFile]) -> Result<()> {
    for file in files {
        let body = ByteStream::from_path(&file.source)
            .await
            .with_context(|| format!("failed to open input file {}", file.source.display()))?;
        let mut upload = client
            .put_object()
            .bucket(bucket)
            .key(&file.key)
            .content_type(&file.content_type)
            .body(body);
        if let Some(cache_control) = file.cache_control {
            upload = upload.cache_control(cache_control);
        }
        upload.send().await.map_err(|error| {
            anyhow!(
                "failed to upload {} to bucket {bucket}: {}",
                file.key,
                DisplayErrorContext(error)
            )
        })?;
    }

    Ok(())
}
