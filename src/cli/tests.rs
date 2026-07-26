use std::path::PathBuf;

use clap::Parser;

use super::{BuildTarget, Cli, Command, DeployTarget};

#[test]
fn parses_build_app_command() {
    let cli = Cli::try_parse_from([
        "cielo",
        "build",
        "app",
        "--data",
        "../weather-data/",
        "--output",
        "dist/application",
    ])
    .expect("build app arguments should parse");

    let Command::Build(args) = cli.command else {
        panic!("build command should be selected");
    };
    let BuildTarget::App(args) = args.target else {
        panic!("app target should be selected");
    };
    assert_eq!(args.data, "../weather-data/");
    assert_eq!(args.output, PathBuf::from("dist/application"));
}

#[test]
fn parses_build_data_command_with_short_option() {
    let cli = Cli::try_parse_from(["cielo", "build", "data", "-o", "dist/weather-data"])
        .expect("build data arguments should parse");

    let Command::Build(args) = cli.command else {
        panic!("build command should be selected");
    };
    let BuildTarget::Data(args) = args.target else {
        panic!("data target should be selected");
    };
    assert_eq!(args.output, PathBuf::from("dist/weather-data"));
}

#[test]
fn parses_deploy_app_command_with_r2_options() {
    let cli = Cli::try_parse_from([
        "cielo",
        "deploy",
        "app",
        "--input",
        "dist/application",
        "--bucket",
        "application",
        "--endpoint",
        "https://account.r2.cloudflarestorage.com",
        "--region",
        "auto",
    ])
    .expect("deploy app arguments should parse");

    let Command::Deploy(args) = cli.command else {
        panic!("deploy command should be selected");
    };
    let DeployTarget::App(args) = args.target else {
        panic!("app target should be selected");
    };
    assert_eq!(args.input, PathBuf::from("dist/application"));
    assert_eq!(args.bucket, "application");
    assert_eq!(
        args.endpoint.as_deref(),
        Some("https://account.r2.cloudflarestorage.com")
    );
    assert_eq!(args.region.as_deref(), Some("auto"));
    assert!(!args.path_style);
}

#[test]
fn parses_deploy_data_command_with_standard_aws_configuration() {
    let cli = Cli::try_parse_from([
        "cielo",
        "deploy",
        "data",
        "-i",
        "dist/weather-data",
        "-b",
        "weather-data",
    ])
    .expect("deploy data arguments should parse");

    let Command::Deploy(args) = cli.command else {
        panic!("deploy command should be selected");
    };
    let DeployTarget::Data(args) = args.target else {
        panic!("data target should be selected");
    };
    assert_eq!(args.input, PathBuf::from("dist/weather-data"));
    assert_eq!(args.bucket, "weather-data");
    assert_eq!(args.endpoint, None);
    assert_eq!(args.region, None);
    assert!(!args.path_style);
}

#[test]
fn parses_path_style_for_a_custom_endpoint() {
    let cli = Cli::try_parse_from([
        "cielo",
        "deploy",
        "data",
        "--input",
        "dist/weather-data",
        "--bucket",
        "weather-data",
        "--endpoint",
        "https://objects.example.test",
        "--path-style",
    ])
    .expect("path-style deploy arguments should parse");

    let Command::Deploy(args) = cli.command else {
        panic!("deploy command should be selected");
    };
    let DeployTarget::Data(args) = args.target else {
        panic!("data target should be selected");
    };
    assert!(args.path_style);
}

#[test]
fn rejects_missing_app_data_url() {
    let error = Cli::try_parse_from(["cielo", "build", "app", "--output", "dist/application"])
        .expect_err("app data URL should be required");

    assert!(error.to_string().contains("--data"));
}

#[test]
fn rejects_missing_data_output_directory() {
    let error = Cli::try_parse_from(["cielo", "build", "data"])
        .expect_err("output directory should be required");

    assert!(error.to_string().contains("--output"));
}

#[test]
fn rejects_missing_deploy_input_or_bucket() {
    for arguments in [
        &["cielo", "deploy", "app", "--bucket", "application"][..],
        &["cielo", "deploy", "data", "--input", "dist/weather-data"][..],
    ] {
        let error =
            Cli::try_parse_from(arguments).expect_err("required deploy option should be rejected");

        assert!(error.to_string().contains("required"));
    }
}

#[test]
fn rejects_removed_commands() {
    for arguments in [
        &["cielo", "generate"][..],
        &["cielo", "generate-data"][..],
        &["cielo", "build", "application"][..],
        &["cielo", "build", "weather-data"][..],
    ] {
        let error = Cli::try_parse_from(arguments).expect_err("removed command should be rejected");

        assert!(error.to_string().contains("unrecognized subcommand"));
    }
}
