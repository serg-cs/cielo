use std::{collections::HashMap, fs, path::Path};

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
fn normalizes_location_names_for_publication() {
    let mut data = sample_data();
    data.municipalities
        .insert("35001".to_owned(), "Arco, El".to_owned());
    data.forecasts[0].province = "Alacant/Alicante".to_owned();

    let snapshot = build_snapshot(data).expect("snapshot should build");

    assert_eq!(snapshot.municipalities[0].name, "El Arco");
    assert_eq!(snapshot.municipalities[0].province, "Alicante");
}

#[test]
fn reorders_only_recognized_deferred_articles() {
    for (source, expected) in [
        ("Arco, El", "El Arco"),
        ("Alcúdia, l'", "l'Alcúdia"),
        ("Cañiza, A", "A Cañiza"),
        (
            "Camp de Mirra, el/Campo de Mirra",
            "el Camp de Mirra/Campo de Mirra",
        ),
        ("Saus, Camallera i Llampaies", "Saus, Camallera i Llampaies"),
        (
            "Cruïlles, Monells i Sant Sadurní de l'Heura",
            "Cruïlles, Monells i Sant Sadurní de l'Heura",
        ),
    ] {
        assert_eq!(normalize_municipality_name(source), expected);
    }
}

#[test]
fn normalizes_aemet_province_qualifiers_and_bilingual_names() {
    for (source, expected) in [
        ("Las Palmas (Gran Canaria)", "Las Palmas"),
        ("Araba/Álava", "Álava"),
        ("València/Valencia", "Valencia"),
        (" Madrid ", "Madrid"),
    ] {
        assert_eq!(normalize_province(source), expected);
    }
}

#[test]
fn publishes_expected_compact_json_layout() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("site-data");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let staging = write_staging_directory(&output_dir, &snapshot, OutputKind::Data)
        .expect("staging should be written");

    publish_staging_directory(&staging, &output_dir, OutputKind::Data)
        .expect("snapshot should publish");

    assert_eq!(
        fs::read_to_string(output_dir.join(MANAGED_MARKER)).expect("marker should be readable"),
        DATA_MARKER_CONTENT
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
    assert_eq!(temperatures["schema_version"], 1);
    assert_eq!(
        temperatures["source"]["generated_at"],
        "2026-07-19T08:00:00"
    );
    assert_eq!(temperatures["municipality_id"], "35001");
    assert_eq!(temperatures["temperatures"][0]["hour"], 10);
    assert_eq!(temperatures["temperatures"][0]["celsius"], 24);
    assert_eq!(temperatures["temperatures"][0]["state"], "cloud-sun");
    assert_eq!(
        temperatures["temperatures"][0]["description"],
        "Intervalos nubosos"
    );
}

#[test]
fn publishes_complete_static_site() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("site");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let staging = write_staging_directory(&output_dir, &snapshot, OutputKind::Site)
        .expect("staging should be written");

    publish_staging_directory(&staging, &output_dir, OutputKind::Site)
        .expect("site should publish");

    assert_eq!(
        fs::read_to_string(output_dir.join(MANAGED_MARKER)).expect("marker should be readable"),
        SITE_MARKER_CONTENT
    );
    assert_eq!(
        fs::read_to_string(output_dir.join(DATA_DIRECTORY).join(MANAGED_MARKER))
            .expect("data marker should be readable"),
        DATA_MARKER_CONTENT
    );
    let index =
        fs::read_to_string(output_dir.join("index.html")).expect("index should be readable");
    assert!(index.contains("<html lang=\"es\">"));
    assert!(index.contains("<cielo-app></cielo-app>"));
    assert!(index.contains("viewport-fit=cover"));
    assert!(index.contains("name=\"theme-color\""));
    assert!(index.contains("rel=\"modulepreload\""));
    assert!(index.contains("type=\"module\" src=\"./assets/site.js\""));
    assert!(output_dir.join("icon.svg").is_file());
    assert!(output_dir.join("assets/site.css").is_file());
    let script =
        fs::read_to_string(output_dir.join("assets/site.js")).expect("script should be readable");
    assert!(script.contains("./components/cielo-icon.js"));
    assert!(script.contains("./components/cielo-app.js"));
    assert!(script.contains("serviceWorker.register"));
    let service_worker = fs::read_to_string(output_dir.join("service-worker.js"))
        .expect("service worker should be readable");
    assert!(service_worker.contains("shell-v1"));
    assert!(service_worker.contains("cacheFirst"));
    assert!(service_worker.contains("`${cachePrefix}data-v1`"));
    assert!(service_worker.contains("./assets/icons.svg"));

    // Recursive embedding must include every dependency of the module entrypoint.
    let app = fs::read_to_string(output_dir.join("assets/components/cielo-app.js"))
        .expect("app component should be readable");
    assert!(app.contains("customElements.define(\"cielo-app\""));
    assert!(app.contains("./data/municipalities.json"));
    for module in [
        "assets/components/cielo-icon.js",
        "assets/components/cielo-locations-view.js",
        "assets/components/cielo-municipality-row.js",
        "assets/components/cielo-municipality-view.js",
        "assets/lib/catalog.js",
        "assets/lib/data-cache.js",
        "assets/lib/storage.js",
        "assets/lib/weather.js",
    ] {
        assert!(output_dir.join(module).is_file(), "missing module {module}");
    }
    let storage = fs::read_to_string(output_dir.join("assets/lib/storage.js"))
        .expect("storage library should be readable");
    assert!(storage.contains("cielo.trackedMunicipalities"));
    assert!(storage.contains("cielo.lastMunicipality"));
    assert_complete_icon_assets(&output_dir, &service_worker);
    assert!(output_dir.join("data/temperatures/35001.json").is_file());
}

fn assert_complete_icon_assets(output_dir: &Path, service_worker: &str) {
    let icon_component = fs::read_to_string(output_dir.join("assets/components/cielo-icon.js"))
        .expect("icon component should be readable");
    assert!(icon_component.contains("new URL(\"../icons.svg\", import.meta.url)"));
    assert!(icon_component.contains("<use></use>"));
    assert!(!icon_component.contains("const iconGlyphs"));
    assert!(!icon_component.contains("<path"));
    assert!(!icon_component.contains("maskImage"));
    let icon_sprite = fs::read_to_string(output_dir.join(ICON_SPRITE_FILENAME))
        .expect("icon sprite should be readable");
    let mut previous_symbol_position = None;
    for icon in [
        "circle-x.svg",
        "cloud-drizzle.svg",
        "cloud-fog.svg",
        "cloud-lightning.svg",
        "cloud-moon-rain.svg",
        "cloud-moon.svg",
        "cloud-rain.svg",
        "cloud-snow.svg",
        "cloud-sun-rain.svg",
        "cloud-sun.svg",
        "cloud.svg",
        "cloudy.svg",
        "list.svg",
        "map-pin.svg",
        "moon.svg",
        "search.svg",
        "snowflake.svg",
        "sun.svg",
        "trash-2.svg",
    ] {
        assert!(
            output_dir.join("assets/icons").join(icon).is_file(),
            "missing icon {icon}"
        );
        assert!(
            !service_worker.contains(&format!("./assets/icons/{icon}")),
            "source icon is still precached separately: {icon}"
        );
        let symbol = format!("<symbol id=\"{}\"", icon.trim_end_matches(".svg"));
        let symbol_position = icon_sprite
            .find(&symbol)
            .unwrap_or_else(|| panic!("icon sprite is missing {symbol}"));
        assert!(
            previous_symbol_position.is_none_or(|previous| previous < symbol_position),
            "icon sprite is not deterministically sorted at {icon}"
        );
        previous_symbol_position = Some(symbol_position);
    }
    assert_eq!(icon_sprite.matches("<symbol id=").count(), 19);
    assert!(output_dir.join("assets/icons/LICENSE").is_file());
}

#[test]
fn builds_icon_symbol_from_canonical_svg() {
    let source = r#"<!-- license -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" stroke="currentColor">
  <path d="M1 2h3" />
</svg>
"#;

    let symbol = build_icon_symbol("sample-icon", source).expect("symbol should build");

    assert!(symbol.starts_with("  <symbol id=\"sample-icon\""));
    assert!(symbol.contains("viewBox=\"0 0 24 24\""));
    assert!(symbol.contains("stroke=\"currentColor\""));
    assert!(symbol.contains("<path d=\"M1 2h3\" />"));
    assert!(!symbol.contains("<svg"));
}

#[test]
fn rejects_invalid_icon_symbols() {
    assert!(build_icon_symbol("../escape", "<svg></svg>").is_err());
    assert!(build_icon_symbol("search", "<svg id=\"duplicate\"></svg>").is_err());
    assert!(build_icon_symbol("search", "<svg><path /></svg>trailing").is_err());
    assert!(build_icon_symbol("search", "<svg><path /></symbol>").is_err());
}

#[test]
fn data_refresh_preserves_site_assets_and_removes_stale_data() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let site_dir = temporary_root.path().join("site");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let site_staging = write_staging_directory(&site_dir, &snapshot, OutputKind::Site)
        .expect("site staging should be written");
    publish_staging_directory(&site_staging, &site_dir, OutputKind::Site)
        .expect("site should publish");
    let original_index =
        fs::read_to_string(site_dir.join("index.html")).expect("index should be readable");
    let data_dir = site_dir.join(DATA_DIRECTORY);
    fs::write(data_dir.join("stale.json"), "old").expect("stale data should be written");

    let data_staging = write_staging_directory(&data_dir, &snapshot, OutputKind::Data)
        .expect("data staging should be written");
    publish_staging_directory(&data_staging, &data_dir, OutputKind::Data)
        .expect("data should publish");

    assert!(!data_dir.join("stale.json").exists());
    assert!(data_dir.join(MUNICIPALITIES_FILENAME).is_file());
    assert_eq!(
        fs::read_to_string(site_dir.join("index.html")).expect("index should remain readable"),
        original_index
    );
    assert_eq!(
        fs::read_to_string(site_dir.join(MANAGED_MARKER)).expect("site marker should be readable"),
        SITE_MARKER_CONTENT
    );
}

#[test]
fn upgrades_legacy_data_snapshot_and_removes_stale_files() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("site-data");
    fs::create_dir(&output_dir).expect("old output should be created");
    fs::write(output_dir.join(MANAGED_MARKER), LEGACY_DATA_MARKER_CONTENT)
        .expect("old marker should be written");
    fs::write(output_dir.join("stale.json"), "old").expect("stale file should be written");
    let snapshot = build_snapshot(sample_data()).expect("snapshot should build");
    let staging = write_staging_directory(&output_dir, &snapshot, OutputKind::Data)
        .expect("staging should be written");

    publish_staging_directory(&staging, &output_dir, OutputKind::Data)
        .expect("snapshot should publish");

    assert!(!output_dir.join("stale.json").exists());
    assert!(output_dir.join(MUNICIPALITIES_FILENAME).is_file());
    assert_eq!(
        fs::read_to_string(output_dir.join(MANAGED_MARKER)).expect("marker should be readable"),
        DATA_MARKER_CONTENT
    );
}

#[test]
fn refuses_to_replace_a_different_output_kind() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let site_dir = temporary_root.path().join("site");
    let data_dir = temporary_root.path().join("data");
    let legacy_dir = temporary_root.path().join("legacy");
    for directory in [&site_dir, &data_dir, &legacy_dir] {
        fs::create_dir(directory).expect("output should be created");
    }
    fs::write(site_dir.join(MANAGED_MARKER), SITE_MARKER_CONTENT)
        .expect("site marker should be written");
    fs::write(data_dir.join(MANAGED_MARKER), DATA_MARKER_CONTENT)
        .expect("data marker should be written");
    fs::write(legacy_dir.join(MANAGED_MARKER), LEGACY_DATA_MARKER_CONTENT)
        .expect("legacy marker should be written");

    let data_error = validate_output_directory(&site_dir, OutputKind::Data)
        .expect_err("site output should not be replaced with data");
    let site_error = validate_output_directory(&data_dir, OutputKind::Site)
        .expect_err("data output should not be replaced with a site");
    let legacy_error = validate_output_directory(&legacy_dir, OutputKind::Site)
        .expect_err("legacy data output should not be replaced with a site");

    assert!(data_error.to_string().contains("not managed as data"));
    assert!(site_error.to_string().contains("not managed as site"));
    assert!(legacy_error.to_string().contains("not managed as site"));
}

#[test]
fn refuses_nonempty_unmanaged_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_dir = temporary_root.path().join("other-data");
    fs::create_dir(&output_dir).expect("output should be created");
    fs::write(output_dir.join("keep.txt"), "important").expect("unmanaged file should be written");

    let error = validate_output_directory(&output_dir, OutputKind::Data)
        .expect_err("unmanaged output should be refused");

    assert!(error.to_string().contains("unmanaged directory"));
    assert_eq!(
        fs::read_to_string(output_dir.join("keep.txt")).expect("file should remain"),
        "important"
    );
}

#[test]
fn refuses_parent_traversal_and_current_directory() {
    let traversal_error =
        validate_output_directory(Path::new("data/../elsewhere"), OutputKind::Data)
            .expect_err("parent traversal should be refused");
    assert!(traversal_error.to_string().contains("cannot contain '..'"));

    let current_error = validate_output_directory(Path::new("."), OutputKind::Data)
        .expect_err("current directory should fail");
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

    let error = validate_output_directory(&output_dir, OutputKind::Data)
        .expect_err("symbolic link output should be refused");

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
            state: crate::aemet::SkyState::CloudSun,
            description: "Intervalos nubosos".to_owned(),
        }],
    }
}
