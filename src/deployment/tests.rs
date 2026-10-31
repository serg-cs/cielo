use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[cfg(unix)]
use std::os::unix::fs::symlink;

use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Credentials, Region, SharedCredentialsProvider, retry::RetryConfig},
};

use crate::cli::{DEFAULT_DEPLOY_CONCURRENCY, DeployTargetArgs};

use super::{
    DATA_CACHE_CONTROL, DeploymentKind, IMMUTABLE_CACHE_CONTROL, REVALIDATE_CACHE_CONTROL,
    create_service_config, list_remote, owned_prefixes, plan_deployment,
    prepare_deployment as prepare, run_bounded_uploads, upload_files,
};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("directory should be created");
    fs::write(path, contents).expect("file should be written");
}

fn data_input(root: &Path) -> PathBuf {
    let input = root.join("data");
    write(&input, "v4/places.json", "{\"schema\":4}");
    write(
        &input,
        "v4/forecast/28/060.json",
        "{\"schema\":4,\"municipalities\":{}}",
    );
    write(&input, "v4/warnings.json", "{\"schema\":4,\"warnings\":[]}");
    write(&input, ".cielo-data", "{\"generator\":\"cielo\"}");
    input
}

#[test]
fn prepares_data_with_places_last_and_without_the_marker() {
    let root = tempfile::tempdir().expect("temporary root");
    let files =
        prepare(&data_input(root.path()), DeploymentKind::Data).expect("data should prepare");
    let keys = files
        .iter()
        .map(|file| file.key.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        keys,
        [
            "v4/forecast/28/060.json",
            "v4/warnings.json",
            "v4/places.json"
        ]
    );
    for file in &files {
        assert_eq!(file.content_type, "application/json");
        assert_eq!(file.cache_control, Some(DATA_CACHE_CONTROL));
    }
    assert_eq!(files[2].body, b"{\"schema\":4}");
}

#[test]
fn plans_only_changed_files_and_scoped_deletions() {
    let root = tempfile::tempdir().expect("temporary root");
    let files = prepare(&data_input(root.path()), DeploymentKind::Data).expect("prepare");
    let prefixes = owned_prefixes(&files, DeploymentKind::Data);
    assert_eq!(
        prefixes,
        ["v4/forecast/", "v4/places.json", "v4/warnings.json"]
    );

    let mut remote = HashMap::new();
    remote.insert(
        "v4/places.json".to_owned(),
        format!("\"{}\"", files[2].md5.to_uppercase()),
    );
    remote.insert("v4/warnings.json".to_owned(), "\"0123\"".to_owned());
    remote.insert("v4/forecast/28/000.json".to_owned(), "\"old\"".to_owned());

    let plan = plan_deployment(files.clone(), Some(&remote), true);
    let uploads = plan
        .uploads
        .iter()
        .map(|file| file.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(uploads, ["v4/forecast/28/060.json", "v4/warnings.json"]);
    assert_eq!(plan.skipped, 1);
    assert_eq!(plan.deletions, ["v4/forecast/28/000.json"]);

    let without_prune = plan_deployment(files.clone(), Some(&remote), false);
    assert_eq!(without_prune.deletions, Vec::<String>::new());
    let without_listing = plan_deployment(files, None, false);
    assert_eq!(without_listing.uploads.len(), 3);
}

#[test]
fn assigns_app_cache_policies_and_entry_points() {
    let root = tempfile::tempdir().expect("temporary root");
    let input = root.path().join("app");
    let hashed = format!("assets/scripts/app.{}.js", "a".repeat(16));
    let legacy = format!("legacy.{}.js", "b".repeat(64));
    let font = format!("assets/fonts/manrope.{}.woff2", "c".repeat(16));
    for path in [
        hashed.as_str(),
        legacy.as_str(),
        font.as_str(),
        "index.html",
        "sw.js",
        "robots.txt",
    ] {
        write(&input, path, "content");
    }

    let files = prepare(&input, DeploymentKind::App).expect("app should prepare");
    let find = |key: &str| {
        files
            .iter()
            .find(|file| file.key == key)
            .expect("file should be prepared")
    };

    assert_eq!(find(&hashed).cache_control, Some(IMMUTABLE_CACHE_CONTROL));
    assert_eq!(find(&legacy).cache_control, Some(IMMUTABLE_CACHE_CONTROL));
    assert_eq!(find(&font).content_type, "font/woff2");
    assert_eq!(
        find("index.html").cache_control,
        Some(REVALIDATE_CACHE_CONTROL)
    );
    assert_eq!(find("sw.js").cache_control, Some(REVALIDATE_CACHE_CONTROL));
    assert_eq!(find("robots.txt").cache_control, None);
    let last = files
        .iter()
        .rev()
        .take(2)
        .map(|file| file.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(last, ["index.html", "sw.js"]);

    let mut remote = HashMap::new();
    remote.insert(hashed.clone(), "\"anything\"".to_owned());
    let plan = plan_deployment(files, Some(&remote), false);
    assert!(
        plan.uploads.iter().all(|file| file.key != hashed),
        "existing hashed assets are kept"
    );
}

#[test]
fn rejects_empty_data_and_app_without_root_index() {
    let root = tempfile::tempdir().expect("temporary root");
    let empty = root.path().join("empty");
    fs::create_dir(&empty).expect("empty directory");
    let error = prepare(&empty, DeploymentKind::Data).expect_err("empty data should fail");
    assert!(error.to_string().contains("contains no files"));

    let app = root.path().join("application");
    write(&app, "nested/index.html", "<html></html>");
    let error = prepare(&app, DeploymentKind::App).expect_err("root index should be required");
    assert!(error.to_string().contains("root index.html"));
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_links_in_input() {
    let root = tempfile::tempdir().expect("temporary root");
    let input = data_input(root.path());
    symlink(input.join("v4/places.json"), input.join("v4/link.json")).expect("symlink");

    let error =
        prepare(&input, DeploymentKind::Data).expect_err("symbolic link should be rejected");
    assert!(error.to_string().contains("symbolic link"));
}

#[tokio::test]
async fn uploads_data_with_headers() {
    let root = tempfile::tempdir().expect("temporary root");
    let input = root.path().join("data");
    write(&input, "v4/places.json", "{}");
    let files = prepare(&input, DeploymentKind::Data).expect("prepare");
    let mut server = mockito::Server::new_async().await;
    let upload = server
        .mock("PUT", "/weather-data/v4/places.json?x-id=PutObject")
        .match_header("content-type", "application/json")
        .match_header("content-encoding", mockito::Matcher::Missing)
        .match_header("cache-control", DATA_CACHE_CONTROL)
        .with_status(200)
        .create_async()
        .await;
    let client = test_client(&server.url());

    upload_files(
        &client,
        "weather-data",
        &files,
        usize::from(DEFAULT_DEPLOY_CONCURRENCY),
    )
    .await
    .expect("upload should succeed");

    upload.assert_async().await;
}

#[tokio::test]
async fn lists_remote_objects_by_prefix() {
    let mut server = mockito::Server::new_async().await;
    let listing = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>weather-data</Name><KeyCount>2</KeyCount><IsTruncated>false</IsTruncated>
  <Contents><Key>v4/places.json</Key><ETag>"abc"</ETag><Size>1</Size></Contents>
  <Contents><Key>v4/places.json.bak</Key><ETag>"def"</ETag><Size>1</Size></Contents>
</ListBucketResult>"#;
    let _list = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/weather-data/?(\?.*)?$".to_owned()),
        )
        .match_query(mockito::Matcher::UrlEncoded(
            "list-type".to_owned(),
            "2".to_owned(),
        ))
        .with_status(200)
        .with_body(listing)
        .create_async()
        .await;
    let client = test_client(&server.url());

    let remote = list_remote(&client, "weather-data", &["v4/places.json".to_owned()])
        .await
        .expect("listing should succeed");

    assert_eq!(remote.len(), 1, "a file prefix only matches that exact key");
    assert_eq!(remote["v4/places.json"], "\"abc\"");
}

#[tokio::test]
async fn stops_before_uploading_index_when_an_asset_fails() {
    let root = tempfile::tempdir().expect("temporary root");
    let input = root.path().join("application");
    write(&input, "app.js", "script");
    write(&input, "index.html", "<html></html>");
    let files = prepare(&input, DeploymentKind::App).expect("prepare");
    let mut server = mockito::Server::new_async().await;
    let bucket = "private-application-bucket";
    let asset = server
        .mock("PUT", format!("/{bucket}/app.js?x-id=PutObject").as_str())
        .with_status(400)
        .with_body("<Error><Code>InvalidRequest</Code></Error>")
        .create_async()
        .await;
    let index = server
        .mock(
            "PUT",
            format!("/{bucket}/index.html?x-id=PutObject").as_str(),
        )
        .expect(0)
        .with_status(200)
        .create_async()
        .await;
    let endpoint = server.url();
    let client = test_client(&endpoint);

    let error = upload_files(
        &client,
        bucket,
        &files,
        usize::from(DEFAULT_DEPLOY_CONCURRENCY),
    )
    .await
    .expect_err("failed asset should stop deployment");

    let message = format!("{error:#}");
    assert!(message.contains("failed to upload app.js"));
    assert!(!message.contains(bucket));
    assert!(!message.contains(&endpoint));
    asset.assert_async().await;
    index.assert_async().await;
}

#[tokio::test]
async fn bounds_the_number_of_uploads_in_progress() {
    for concurrency in [1, 3] {
        let in_progress = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let operation_in_progress = Arc::clone(&in_progress);
        let operation_maximum = Arc::clone(&maximum);

        run_bounded_uploads(0..8, concurrency, move |_| {
            let in_progress = Arc::clone(&operation_in_progress);
            let maximum = Arc::clone(&operation_maximum);
            async move {
                let current = in_progress.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::task::yield_now().await;
                in_progress.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await
        .expect("bounded uploads should finish");

        assert_eq!(maximum.load(Ordering::SeqCst), concurrency);
        assert_eq!(in_progress.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn stops_scheduling_uploads_after_a_failure() {
    let started = Arc::new(AtomicUsize::new(0));
    let operation_started = Arc::clone(&started);

    let error = run_bounded_uploads(0..4, 1, move |item| {
        let started = Arc::clone(&operation_started);
        async move {
            started.fetch_add(1, Ordering::SeqCst);
            if item == 0 {
                anyhow::bail!("expected upload failure");
            }

            Ok(())
        }
    })
    .await
    .expect_err("the first upload should fail");

    assert!(error.to_string().contains("expected upload failure"));
    assert_eq!(started.load(Ordering::SeqCst), 1);
}

fn test_client(endpoint: &str) -> Client {
    let shared_config = aws_config::SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("auto"))
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            "test-access-key",
            "test-secret-key",
            None,
            None,
            "deployment-test",
        )))
        .retry_config(RetryConfig::disabled())
        .build();
    let args = DeployTargetArgs {
        input: PathBuf::new(),
        bucket: "test".to_owned(),
        concurrency: DEFAULT_DEPLOY_CONCURRENCY,
        endpoint: Some(endpoint.to_owned()),
        region: None,
        path_style: true,
        prune: false,
        dry_run: false,
    };

    Client::from_conf(create_service_config(&shared_config, &args))
}
