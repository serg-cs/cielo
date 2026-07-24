use std::path::PathBuf;

use clap::Parser;

use super::{BuildTarget, Cli, Command};

#[test]
fn parses_build_app_command() {
    let cli = Cli::try_parse_from([
        "cielo",
        "build",
        "app",
        "--data-url",
        "../data/",
        "--output-dir",
        "build/app",
    ])
    .expect("build app arguments should parse");

    let Command::Build(args) = cli.command;
    let BuildTarget::App(args) = args.target else {
        panic!("app build target should be selected");
    };
    assert_eq!(args.data_url, "../data/");
    assert_eq!(args.output_dir, PathBuf::from("build/app"));
}

#[test]
fn parses_build_data_command() {
    let cli = Cli::try_parse_from(["cielo", "build", "data", "--output-dir", "build/data"])
        .expect("build data arguments should parse");

    let Command::Build(args) = cli.command;
    let BuildTarget::Data(args) = args.target else {
        panic!("data build target should be selected");
    };
    assert_eq!(args.output_dir, PathBuf::from("build/data"));
}

#[test]
fn rejects_missing_app_data_url() {
    let error = Cli::try_parse_from(["cielo", "build", "app", "--output-dir", "build/app"])
        .expect_err("app data URL should be required");

    assert!(error.to_string().contains("--data-url"));
}

#[test]
fn rejects_missing_data_output_directory() {
    let error = Cli::try_parse_from(["cielo", "build", "data"])
        .expect_err("output directory should be required");

    assert!(error.to_string().contains("--output-dir"));
}

#[test]
fn rejects_removed_generate_commands() {
    for command in ["generate", "generate-data"] {
        let error = Cli::try_parse_from(["cielo", command])
            .expect_err("removed generate command should be rejected");

        assert!(error.to_string().contains("unrecognized subcommand"));
    }
}
