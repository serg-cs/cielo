use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn build_app_does_not_require_api_key_environment_variable() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let mut command = cargo_bin_cmd!("cielo");
    let output_dir = temporary_root.path().join("application");

    command
        .env_remove("AEMET_API_KEY")
        .arg("build")
        .arg("app")
        .arg("--data")
        .arg("../weather-data/")
        .arg("--output")
        .arg(&output_dir)
        .assert()
        .success();

    assert!(output_dir.join("index.html").is_file());
    assert!(!output_dir.join("weather-data").exists());
}

#[test]
fn build_data_requires_api_key_environment_variable() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let mut command = cargo_bin_cmd!("cielo");

    command
        .env_remove("AEMET_API_KEY")
        .arg("build")
        .arg("data")
        .arg("--output")
        .arg(temporary_root.path().join("weather-data"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AEMET_API_KEY environment variable is not set",
        ));
}

#[test]
fn help_describes_only_the_build_command() {
    let mut command = cargo_bin_cmd!("cielo");

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("generate").not());

    let mut build_command = cargo_bin_cmd!("cielo");
    build_command
        .arg("build")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("app"))
        .stdout(predicate::str::contains("data"));
}

#[test]
fn removed_generate_commands_are_rejected() {
    for command in ["generate", "generate-data"] {
        let mut cielo = cargo_bin_cmd!("cielo");
        cielo
            .arg(command)
            .assert()
            .failure()
            .stderr(predicate::str::contains("unrecognized subcommand"));
    }
}

#[test]
fn verbose_build_commands_are_rejected() {
    for target in ["application", "weather-data"] {
        let mut cielo = cargo_bin_cmd!("cielo");
        cielo
            .arg("build")
            .arg(target)
            .assert()
            .failure()
            .stderr(predicate::str::contains("unrecognized subcommand"));
    }
}

#[test]
fn version_reports_v1_release() {
    let mut command = cargo_bin_cmd!("cielo");

    command
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("cielo 1.0.0"));
}
