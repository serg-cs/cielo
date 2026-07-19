use std::path::PathBuf;

use clap::Parser;

use super::{Cli, Command};

#[test]
fn parses_generate_command() {
    let cli = Cli::try_parse_from(["cielo", "generate", "--output-dir", "build/data"])
        .expect("generate arguments should parse");

    let Command::Generate(args) = cli.command;
    assert_eq!(args.output_dir, PathBuf::from("build/data"));
}

#[test]
fn rejects_missing_output_directory() {
    let error = Cli::try_parse_from(["cielo", "generate"])
        .expect_err("output directory should be required");

    assert!(error.to_string().contains("--output-dir"));
}
