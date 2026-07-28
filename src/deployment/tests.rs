use std::{fs, path::PathBuf};

#[cfg(unix)]
use std::os::unix::fs::symlink;

use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Credentials, Region, SharedCredentialsProvider, retry::RetryConfig},
};

use crate::cli::DeployTargetArgs;

use super::{
    DeploymentKind, IMMUTABLE_CACHE_CONTROL, create_service_config, prepare_deployment,
    upload_files,
};

#[test]
fn prepares_recursive_app_deployment_with_entry_point_last() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("application");
    fs::create_dir_all(input.join("assets/scripts")).expect("asset directory should be created");
    fs::write(input.join("index.html"), "<html></html>").expect("index should be created");
    fs::write(input.join(".headers"), "headers").expect("hidden file should be created");
    fs::write(input.join("assets/scripts/app.js"), "script").expect("script should be created");

    let files =
        prepare_deployment(&input, DeploymentKind::App).expect("app deployment should prepare");
    let keys = files
        .iter()
        .map(|file| file.key.as_str())
        .collect::<Vec<_>>();

    assert_eq!(keys, [".headers", "assets/scripts/app.js", "index.html"]);
    assert_eq!(files[1].content_type, "text/javascript");
    assert_eq!(files[2].content_type, "text/html");
}

#[test]
fn prepares_data_deployment_with_catalog_last() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("weather-data");
    fs::create_dir_all(input.join("forecasts/35")).expect("forecast directory should be created");
    fs::write(input.join("catalog.json"), "{}").expect("catalog should be created");
    fs::write(input.join("forecasts/35/000"), "forecast")
        .expect("extensionless forecast should be created");

    let files =
        prepare_deployment(&input, DeploymentKind::Data).expect("data deployment should prepare");

    assert_eq!(files[0].key, "forecasts/35/000");
    assert_eq!(files[0].content_type, "application/octet-stream");
    assert_eq!(files[1].key, "catalog.json");
    assert_eq!(files[1].content_type, "application/json");
}

#[test]
fn assigns_immutable_caching_only_to_content_hashed_files() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("application");
    fs::create_dir(&input).expect("app directory should be created");
    let hashed_key = format!("app.{}.js", "a".repeat(16));
    let legacy_hashed_key = format!("legacy.{}.js", "b".repeat(64));
    fs::write(input.join(&hashed_key), "script").expect("hashed script should be created");
    fs::write(input.join(&legacy_hashed_key), "legacy script")
        .expect("legacy hashed script should be created");
    fs::write(input.join("index.html"), "<html></html>").expect("index should be created");
    fs::write(
        input.join(format!("uppercase.{}.js", "A".repeat(16))),
        "script",
    )
    .expect("uppercase hash script should be created");
    fs::write(input.join(format!("short.{}.js", "a".repeat(15))), "script")
        .expect("wrong-length hash script should be created");

    let files =
        prepare_deployment(&input, DeploymentKind::App).expect("app deployment should prepare");
    let hashed_file = files
        .iter()
        .find(|file| file.key == hashed_key)
        .expect("hashed script should be prepared");
    let legacy_hashed_file = files
        .iter()
        .find(|file| file.key == legacy_hashed_key)
        .expect("legacy hashed script should be prepared");

    assert_eq!(hashed_file.cache_control, Some(IMMUTABLE_CACHE_CONTROL));
    assert_eq!(
        legacy_hashed_file.cache_control,
        Some(IMMUTABLE_CACHE_CONTROL)
    );
    assert!(
        files
            .iter()
            .filter(|file| { file.key != hashed_file.key && file.key != legacy_hashed_file.key })
            .all(|file| file.cache_control.is_none())
    );
}

#[test]
fn prepares_hashed_woff2_with_font_content_type_and_immutable_caching() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("application");
    fs::create_dir_all(input.join("assets/fonts")).expect("font directory should be created");
    fs::write(input.join("index.html"), "<html></html>").expect("index should be created");
    let font_key = format!("assets/fonts/manrope.{}.woff2", "a".repeat(16));
    fs::write(input.join(&font_key), "font").expect("font should be created");

    let files =
        prepare_deployment(&input, DeploymentKind::App).expect("app deployment should prepare");
    let font = files
        .iter()
        .find(|file| file.key == font_key)
        .expect("font should be prepared");

    assert_eq!(font.content_type, "font/woff2");
    assert_eq!(font.cache_control, Some(IMMUTABLE_CACHE_CONTROL));
}

#[test]
fn rejects_empty_data_and_app_without_root_index() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let empty = temporary_root.path().join("empty");
    fs::create_dir(&empty).expect("empty directory should be created");
    let empty_error = prepare_deployment(&empty, DeploymentKind::Data)
        .expect_err("empty data deployment should fail");
    assert!(empty_error.to_string().contains("contains no files"));

    let app = temporary_root.path().join("application");
    fs::create_dir_all(app.join("nested")).expect("nested directory should be created");
    fs::write(app.join("nested/index.html"), "<html></html>")
        .expect("nested index should be created");
    let index_error =
        prepare_deployment(&app, DeploymentKind::App).expect_err("root index should be required");
    assert!(index_error.to_string().contains("root index.html"));
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_links_in_input() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("weather-data");
    fs::create_dir(&input).expect("data directory should be created");
    fs::write(input.join("catalog.json"), "{}").expect("catalog should be created");
    symlink(input.join("catalog.json"), input.join("catalog-link.json"))
        .expect("catalog symlink should be created");

    let error = prepare_deployment(&input, DeploymentKind::Data)
        .expect_err("symbolic link should be rejected");

    assert!(error.to_string().contains("symbolic link"));
}

#[tokio::test]
async fn uploads_the_same_key_with_content_type_on_each_deployment() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("weather-data");
    fs::create_dir(&input).expect("data directory should be created");
    fs::write(input.join("catalog.json"), "{}").expect("catalog should be created");
    let files =
        prepare_deployment(&input, DeploymentKind::Data).expect("data deployment should prepare");
    let mut server = mockito::Server::new_async().await;
    let upload = server
        .mock("PUT", "/weather-data/catalog.json?x-id=PutObject")
        .match_header("content-type", "application/json")
        .expect(2)
        .with_status(200)
        .create_async()
        .await;
    let client = test_client(&server.url());

    upload_files(&client, "weather-data", &files)
        .await
        .expect("first deployment should upload");
    upload_files(&client, "weather-data", &files)
        .await
        .expect("second deployment should overwrite the same key");

    upload.assert_async().await;
}

#[tokio::test]
async fn uploads_content_hashed_files_with_immutable_caching() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("application");
    fs::create_dir(&input).expect("app directory should be created");
    let hash = "a".repeat(16);
    let asset_key = format!("app.{hash}.js");
    fs::write(input.join(&asset_key), "script").expect("hashed script should be created");
    fs::write(input.join("index.html"), "<html></html>").expect("index should be created");
    let files =
        prepare_deployment(&input, DeploymentKind::App).expect("app deployment should prepare");
    let mut server = mockito::Server::new_async().await;
    let asset = server
        .mock(
            "PUT",
            format!("/application/{asset_key}?x-id=PutObject").as_str(),
        )
        .match_header("cache-control", IMMUTABLE_CACHE_CONTROL)
        .with_status(200)
        .create_async()
        .await;
    let index = server
        .mock("PUT", "/application/index.html?x-id=PutObject")
        .match_header("cache-control", mockito::Matcher::Missing)
        .with_status(200)
        .create_async()
        .await;
    let client = test_client(&server.url());

    upload_files(&client, "application", &files)
        .await
        .expect("application files should upload");

    asset.assert_async().await;
    index.assert_async().await;
}

#[tokio::test]
async fn stops_before_uploading_index_when_an_asset_fails() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let input = temporary_root.path().join("application");
    fs::create_dir(&input).expect("app directory should be created");
    fs::write(input.join("app.js"), "script").expect("script should be created");
    fs::write(input.join("index.html"), "<html></html>").expect("index should be created");
    let files =
        prepare_deployment(&input, DeploymentKind::App).expect("app deployment should prepare");
    let mut server = mockito::Server::new_async().await;
    let bucket = "private-application-bucket";
    let asset = server
        .mock(
            "PUT",
            format!("/{bucket}/app.js?x-id=PutObject").as_str(),
        )
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

    let error = upload_files(&client, bucket, &files)
        .await
        .expect_err("failed asset should stop deployment");

    let message = format!("{error:#}");
    assert!(message.contains("failed to upload app.js"));
    assert!(!message.contains(bucket));
    assert!(!message.contains(&endpoint));
    asset.assert_async().await;
    index.assert_async().await;
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
        endpoint: Some(endpoint.to_owned()),
        region: None,
        path_style: true,
    };

    Client::from_conf(create_service_config(&shared_config, &args))
}
