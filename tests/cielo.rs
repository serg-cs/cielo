use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn generate_requires_api_key_environment_variable() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let mut command = cargo_bin_cmd!("cielo");

    command
        .env_remove("AEMET_API_KEY")
        .arg("generate")
        .arg("--output-dir")
        .arg(temporary_root.path().join("data"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AEMET_API_KEY environment variable is not set",
        ));
}

#[test]
fn generate_data_requires_api_key_environment_variable() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let mut command = cargo_bin_cmd!("cielo");

    command
        .env_remove("AEMET_API_KEY")
        .arg("generate-data")
        .arg("--output-dir")
        .arg(temporary_root.path().join("data"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AEMET_API_KEY environment variable is not set",
        ));
}

#[test]
fn help_describes_the_generate_command() {
    let mut command = cargo_bin_cmd!("cielo");

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("generate"))
        .stdout(predicate::str::contains("generate-data"));
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
