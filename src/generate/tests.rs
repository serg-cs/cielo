use std::{collections::HashMap, fs};

use serde_json::Value;

use super::*;

#[test]
fn builds_municipalities_from_forecast_ids_with_master_name_overlay() {
    let mut data = sample_data();
    data.municipalities
        .insert("36011".to_owned(), "Master only".to_owned());
    data.forecasts
        .push(forecast("51001", "Forecast fallback", "Ceuta", 25));

    let snapshot = build_snapshot(data).expect("snapshot should build");

    assert_eq!(snapshot.municipalities.len(), 2);
    assert_eq!(snapshot.municipalities[0].id, "35001");
    assert_eq!(snapshot.municipalities[0].name, "Master name");
    assert_eq!(snapshot.municipalities[0].province, "Las Palmas");
    assert_eq!(
        snapshot.municipalities[0].timezone,
        Timezone::AtlanticCanary
    );
    assert_eq!(snapshot.municipalities[1].id, "51001");
    assert_eq!(snapshot.municipalities[1].name, "Forecast fallback");
    assert_eq!(snapshot.municipalities[1].timezone, Timezone::AfricaCeuta);
}

#[test]
fn publishes_expected_compact_json_layout() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("site-data");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let staging =
        write_staging_directory(&output_dir, &snapshot).expect("staging should be written");

    publish_staging_directory(&staging, &output_dir).expect("snapshot should publish");

    assert_eq!(
        fs::read_to_string(output_dir.join(MANAGED_MARKER)).expect("marker should be readable"),
        MANAGED_MARKER_CONTENT
    );
    let municipalities_text = fs::read_to_string(output_dir.join(MUNICIPALITIES_FILENAME))
        .expect("municipalities should be readable");
    assert!(municipalities_text.ends_with('\n'));
    assert!(!municipalities_text.contains("\n  "));
    let municipalities: Value =
        serde_json::from_str(&municipalities_text).expect("municipalities should be JSON");
    assert_eq!(municipalities["schema_version"], 1);
    assert_eq!(municipalities["source"]["name"], "AEMET");
    assert_eq!(municipalities["municipalities"][0]["id"], "35001");
    assert_eq!(
        municipalities["municipalities"][0]["timezone"],
        "Atlantic/Canary"
    );

    let temperature_path = output_dir.join("temperatures/35001.json");
    let temperatures: Value = serde_json::from_str(
        &fs::read_to_string(temperature_path).expect("temperatures should be readable"),
    )
    .expect("temperatures should be JSON");
    assert_eq!(
        temperatures["source"]["generated_at"],
        "2026-07-19T08:00:00"
    );
    assert_eq!(temperatures["municipality_id"], "35001");
    assert_eq!(temperatures["temperatures"][0]["hour"], 10);
    assert_eq!(temperatures["temperatures"][0]["celsius"], 24);
}

#[test]
fn replaces_managed_snapshot_and_removes_stale_files() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("site-data");
    fs::create_dir(&output_dir).expect("old output should be created");
    fs::write(output_dir.join(MANAGED_MARKER), MANAGED_MARKER_CONTENT)
        .expect("old marker should be written");
    fs::write(output_dir.join("stale.json"), "old").expect("stale file should be written");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let staging =
        write_staging_directory(&output_dir, &snapshot).expect("staging should be written");

    publish_staging_directory(&staging, &output_dir).expect("snapshot should publish");

    assert!(!output_dir.join("stale.json").exists());
    assert!(output_dir.join(MUNICIPALITIES_FILENAME).is_file());
}

#[test]
fn refuses_nonempty_unmanaged_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("other-data");
    fs::create_dir(&output_dir).expect("output should be created");
    fs::write(output_dir.join("keep.txt"), "important").expect("unmanaged file should be written");

    let error =
        validate_output_directory(&output_dir).expect_err("unmanaged output should be refused");

    assert!(error.to_string().contains("unmanaged directory"));
    assert_eq!(
        fs::read_to_string(output_dir.join("keep.txt")).expect("file should remain"),
        "important"
    );
}

#[test]
fn refuses_parent_traversal_and_current_directory() {
    let traversal_error = validate_output_directory(Path::new("data/../elsewhere"))
        .expect_err("parent traversal should be refused");
    assert!(traversal_error.to_string().contains("cannot contain '..'"));

    let current_error =
        validate_output_directory(Path::new(".")).expect_err("current directory should fail");
    assert!(current_error.to_string().contains("current directory"));
}

#[cfg(unix)]
#[test]
fn refuses_symbolic_link_output() {
    use std::os::unix::fs::symlink;

    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let real_dir = temporary_root.path().join("real");
    let output_dir = temporary_root.path().join("linked");
    fs::create_dir(&real_dir).expect("real directory should be created");
    symlink(&real_dir, &output_dir).expect("symbolic link should be created");

    let error =
        validate_output_directory(&output_dir).expect_err("symbolic link output should be refused");

    assert!(error.to_string().contains("symbolic link"));
}

fn sample_data() -> AemetData {
    AemetData {
        municipalities: HashMap::from([("35001".to_owned(), "Master name".to_owned())]),
        forecasts: vec![forecast(
            "35001",
            "Forecast name",
            "Las Palmas (Gran Canaria)",
            24,
        )],
    }
}

fn forecast(id: &str, name: &str, province: &str, celsius: i16) -> Forecast {
    Forecast {
        id: id.to_owned(),
        name: name.to_owned(),
        province: province.to_owned(),
        generated_at: "2026-07-19T08:00:00".to_owned(),
        temperatures: vec![Temperature {
            date: "2026-07-19".to_owned(),
            hour: 10,
            celsius,
        }],
    }
}
