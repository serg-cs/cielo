use std::path::PathBuf;

use clap::Parser;

use super::{BuildTarget, Cli, Command};

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

    let Command::Build(args) = cli.command;
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

    let Command::Build(args) = cli.command;
    let BuildTarget::Data(args) = args.target else {
        panic!("data target should be selected");
    };
    assert_eq!(args.output, PathBuf::from("dist/weather-data"));
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
