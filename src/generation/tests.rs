use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::aemet::{AemetWeatherData, HourlyForecast, MunicipalityForecast, WeatherCondition};
use html5ever::{parse_document, tendril::TendrilSink};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use super::{
    application::{build_icon_symbol, generate_application, normalize_weather_data_url},
    files::GeneratedFiles,
    publisher::{OutputKind, create_staging_directory, publish_staging_directory},
    weather_data::{WeatherDataStatistics, build_snapshot, write_weather_data_files},
};

#[test]
fn normalizes_supported_weather_data_urls() {
    for (source, expected) in [
        (
            "https://data.example.test/weather",
            "https://data.example.test/weather/",
        ),
        (
            "http://localhost:9000/weather-data/",
            "http://localhost:9000/weather-data/",
        ),
        ("/weather-data", "/weather-data/"),
        ("../weather-data", "../weather-data/"),
    ] {
        assert_eq!(
            normalize_weather_data_url(source).expect("URL should be valid"),
            expected
        );
    }
}

#[test]
fn rejects_unsupported_weather_data_urls() {
    for value in [
        "",
        "ftp://data.example.test/weather",
        "//data.example.test/weather",
        "https://user@example.test/weather",
        "https://data.example.test/weather?version=1",
        r"..\weather-data",
    ] {
        assert!(normalize_weather_data_url(value).is_err());
    }
}

#[test]
fn converts_canonical_svg_to_named_symbol() {
    let source = r#"<svg width="24" height='24' viewBox="0 0 24 24" stroke="currentColor"><path d="M1 2h3" /></svg>"#;

    let symbol = build_icon_symbol("sample-icon", source).expect("symbol should build");

    assert!(symbol.starts_with("<symbol id=\"cielo-icon-sample-icon\""));
    assert!(!symbol.contains("width="));
    assert!(!symbol.contains("height="));
    assert!(symbol.contains("viewBox=\"0 0 24 24\""));
    assert!(symbol.contains("<path d=\"M1 2h3\" />"));
    assert!(symbol.ends_with("</symbol>"));
}

#[test]
fn rejects_unsafe_icon_sources() {
    for (name, source) in [
        ("Invalid Name", "<svg></svg>"),
        ("valid-name", "<svg id=\"source-id\"></svg>"),
        ("valid-name", "<svg></svg>trailing content"),
        ("valid-name", "<svg>"),
        ("valid-name", "<svg width=24></svg>"),
    ] {
        assert!(build_icon_symbol(name, source).is_err());
    }
}

#[test]
fn generated_files_track_content_and_reject_unsafe_paths() {
    let mut files = GeneratedFiles::default();
    files
        .insert("z.js", b"z".to_vec())
        .expect("path should work");
    files
        .insert("a.css", b"a".to_vec())
        .expect("path should work");

    assert_eq!(files.file_count(), 2);
    assert_eq!(files.total_bytes(), 2);
    assert!(files.insert(Path::new("../escape"), Vec::new()).is_err());
    assert!(files.insert(Path::new("/absolute"), Vec::new()).is_err());
}

#[test]
fn refuses_to_replace_unmanaged_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    fs::create_dir(&output_directory).expect("output directory should be created");
    fs::write(output_directory.join("personal-file.txt"), "keep")
        .expect("unmanaged file should be created");

    let error = create_staging_directory(&output_directory, OutputKind::App)
        .expect_err("unmanaged output should be rejected");

    assert!(error.to_string().contains("refusing to replace"));
    assert_eq!(
        fs::read_to_string(output_directory.join("personal-file.txt"))
            .expect("unmanaged file should remain"),
        "keep"
    );
}

#[test]
fn refuses_to_replace_app_output_with_data() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    generate_application(&output_directory, "../weather-data")
        .expect("application should generate");

    let error = create_staging_directory(&output_directory, OutputKind::Data)
        .expect_err("different output kind should be rejected");

    assert!(
        error
            .to_string()
            .contains("not recognized as Cielo data output")
    );
    assert!(output_directory.join("index.html").is_file());
}

#[test]
fn replaces_existing_generated_data_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("data");
    fs::create_dir(&output_directory).expect("output directory should be created");
    fs::write(
        output_directory.join("municipalities.json"),
        r#"{"generator":"cielo","source":{"name":"AEMET","url":"https://opendata.aemet.es/"},"municipalities":[]}"#,
    )
    .expect("catalog should be created");
    fs::write(output_directory.join("stale.json"), "{}").expect("stale file should be created");

    let (output_directory, staging) = create_staging_directory(&output_directory, OutputKind::Data)
        .expect("generated data output should be recognized");
    fs::write(
        staging.path().join("municipalities.json"),
        r#"{"generator":"cielo","source":{"name":"AEMET","url":"https://opendata.aemet.es/"},"municipalities":[]}"#,
    )
    .expect("replacement catalog should be created");
    publish_staging_directory(&staging, &output_directory, OutputKind::Data)
        .expect("generated data output should be replaced");

    assert!(!output_directory.join("stale.json").exists());
    assert!(output_directory.join("municipalities.json").is_file());
}

#[test]
fn replaces_existing_generated_app_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    generate_application(&output_directory, "../weather-data")
        .expect("initial app should generate");
    fs::write(output_directory.join("stale.js"), "stale").expect("stale file should be created");

    generate_application(&output_directory, "../weather-data")
        .expect("generated app should be replaced");

    assert!(!output_directory.join("stale.js").exists());
    assert!(output_directory.join("index.html").is_file());
}

#[test]
fn rejects_unsafe_output_directories() {
    assert!(create_staging_directory(Path::new("."), OutputKind::App).is_err());
    assert!(
        create_staging_directory(Path::new("generated/../application"), OutputKind::App,).is_err()
    );
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_link_output_directory() {
    use std::os::unix::fs::symlink;

    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let target = temporary_root.path().join("target");
    let output_directory = temporary_root.path().join("application");
    fs::create_dir(&target).expect("symlink target should be created");
    symlink(&target, &output_directory).expect("output symlink should be created");

    let error = create_staging_directory(&output_directory, OutputKind::App)
        .expect_err("symlink output should be rejected");

    assert!(error.to_string().contains("symbolic link"));
}

#[cfg(unix)]
#[test]
fn rejects_output_with_a_symbolic_link_identity_file() {
    use std::os::unix::fs::symlink;

    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("app");
    let external_index = temporary_root.path().join("external-index.html");
    fs::create_dir(&output_directory).expect("output directory should be created");
    fs::write(
        &external_index,
        r#"<meta name="generator" content="cielo">"#,
    )
    .expect("external index should be created");
    symlink(&external_index, output_directory.join("index.html"))
        .expect("identity symlink should be created");

    let error = create_staging_directory(&output_directory, OutputKind::App)
        .expect_err("identity symlink should be rejected");

    assert!(error.to_string().contains("not recognized"));
}

#[test]
fn generates_exact_readable_app_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");

    let summary = generate_application(&output_directory, "../weather-data")
        .expect("application should generate");

    let paths = output_paths(&output_directory);
    assert_eq!(paths.len(), summary.files);
    for expected in [
        "index.html",
        "manifest.webmanifest",
        "favicon.svg",
        "assets/styles/design-tokens.css",
        "assets/styles/foundation.css",
        "assets/styles/locations.css",
        "assets/styles/forecast.css",
        "assets/styles/interactions.css",
        "assets/scripts/main.js",
        "assets/scripts/application-controller.js",
        "assets/scripts/locations-controller.js",
        "assets/scripts/forecast-controller.js",
        "assets/scripts/municipality-row-gesture-controller.js",
        "assets/scripts/municipality-catalog.js",
        "assets/scripts/weather-data-client.js",
        "assets/scripts/forecast-store.js",
        "assets/scripts/preferences-store.js",
        "assets/scripts/dom.js",
    ] {
        assert!(paths.contains(&expected.to_owned()), "missing {expected}");
    }
    assert!(!paths.contains(&"service-worker.js".to_owned()));
    assert!(paths.iter().all(|path| !path.starts_with('.')));
    assert!(paths.iter().all(|path| !path.contains(".DS_Store")));
    assert!(
        paths
            .iter()
            .all(|path| !has_extension(path, "svg") || path == "favicon.svg")
    );
    assert!(summary.bytes <= 185_000);

    let index =
        fs::read_to_string(output_directory.join("index.html")).expect("index should be readable");
    let dom = parse_document(RcDom::default(), html5ever::ParseOpts::default()).one(index.clone());
    assert!(
        dom.errors.borrow().is_empty(),
        "generated HTML contains parse errors"
    );
    assert_generated_document_invariants(&dom);
    assert!(index.contains("<template id=\"municipality-row-template\">"));
    assert!(index.contains(r#"<meta name="generator" content="cielo">"#));
    assert!(index.contains("id=\"cielo-application\""));
    assert!(index.contains("data-weather-data-url=\"../weather-data/\""));
    assert!(index.contains("<symbol id=\"cielo-icon-sun\""));
    assert!(!index.contains("back-swipe-region"));

    for stylesheet in [
        "design-tokens.css",
        "foundation.css",
        "locations.css",
        "forecast.css",
        "interactions.css",
    ] {
        let css = fs::read_to_string(output_directory.join("assets/styles").join(stylesheet))
            .expect("stylesheet should be readable");
        StyleSheet::parse(&css, ParserOptions::default())
            .unwrap_or_else(|error| panic!("invalid CSS in {stylesheet}: {error:?}"));
    }

    let mut javascript_bytes = 0;
    for path in paths.iter().filter(|path| has_extension(path, "js")) {
        let script =
            fs::read_to_string(output_directory.join(path)).expect("script should be readable");
        assert!(!script.contains("innerHTML"));
        assert!(!script.contains("<style>"));
        assert!(!script.contains("customElements"));
        javascript_bytes += script.len();
    }
    assert!(javascript_bytes <= 95_000);
}

#[test]
fn app_generation_is_deterministic() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let first_directory = temporary_root.path().join("first");
    let second_directory = temporary_root.path().join("second");

    generate_application(&first_directory, "../weather-data").expect("first app should generate");
    generate_application(&second_directory, "../weather-data").expect("second app should generate");

    assert_eq!(
        output_paths(&first_directory),
        output_paths(&second_directory)
    );
    for path in output_paths(&first_directory) {
        assert_eq!(
            fs::read(first_directory.join(&path)).expect("first file should read"),
            fs::read(second_directory.join(&path)).expect("second file should read"),
            "different generated bytes for {path}"
        );
    }
}

#[test]
fn builds_weather_snapshot() {
    let snapshot = build_snapshot(sample_source_data()).expect("snapshot should build");

    assert_eq!(snapshot.municipalities.len(), 1);
    assert_eq!(snapshot.municipalities[0].id, "35001");
    assert_eq!(snapshot.municipalities[0].name, "El Arco");
    assert_eq!(snapshot.municipalities[0].province, "Las Palmas");
    assert_eq!(snapshot.forecasts[0].hourly_forecasts.len(), 1);

    let serialized = serde_json::to_value(&snapshot.forecasts[0].hourly_forecasts[0])
        .expect("hourly forecast should serialize");
    assert_eq!(serialized["temperature_celsius"], 24);
    assert_eq!(serialized["condition"], "cloud-sun");
    assert!(serialized.get("celsius").is_none());
    assert!(serialized.get("state").is_none());
}

#[test]
fn writes_readable_weather_data_files() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let snapshot = build_snapshot(sample_source_data()).expect("snapshot should build");
    let mut statistics = WeatherDataStatistics::default();

    write_weather_data_files(temporary_root.path(), &snapshot, &mut statistics)
        .expect("weather-data files should write");

    assert_eq!(
        output_paths(temporary_root.path()),
        ["hourly_forecasts/35001.json", "municipalities.json"]
    );
    let catalog: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary_root.path().join("municipalities.json"))
            .expect("catalog should be readable"),
    )
    .expect("catalog should decode");
    let forecast: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary_root.path().join("hourly_forecasts/35001.json"))
            .expect("forecast should be readable"),
    )
    .expect("forecast should decode");

    assert_eq!(catalog["generator"], "cielo");
    assert!(catalog.get("schema_version").is_none());
    assert_eq!(catalog["source"]["generated_at"], "2026-07-25T08:00:00");
    assert_eq!(catalog["municipalities"][0]["time_zone"], "Atlantic/Canary");
    assert_eq!(forecast["generator"], "cielo");
    assert!(forecast.get("schema_version").is_none());
    assert_eq!(forecast["municipality_id"], "35001");
    assert_eq!(forecast["hourly_forecasts"][0]["temperature_celsius"], 24);
    assert!(forecast.get("temperatures").is_none());
}

#[test]
fn weather_snapshot_rejects_duplicate_forecasts() {
    let mut source_data = sample_source_data();
    source_data.forecasts.push(source_data.forecasts[0].clone());

    let error = build_snapshot(source_data).expect_err("duplicate forecast should fail");

    assert!(error.to_string().contains("duplicate forecast ID"));
}

fn output_paths(root: &Path) -> Vec<String> {
    fn collect(root: &Path, directory: &Path, paths: &mut Vec<String>) {
        for entry in fs::read_dir(directory).expect("output directory should be readable") {
            let entry = entry.expect("output entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, paths);
            } else {
                paths.push(
                    path.strip_prefix(root)
                        .expect("path should be relative")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    let mut paths = Vec::new();
    collect(root, root, &mut paths);
    paths.sort();
    paths
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

#[derive(Default)]
struct GeneratedDocumentInvariants {
    ids: HashSet<String>,
    classes: HashSet<String>,
    use_targets: Vec<String>,
    symbol_count: usize,
}

fn assert_generated_document_invariants(dom: &RcDom) {
    let mut invariants = GeneratedDocumentInvariants::default();
    collect_document_invariants(&dom.document, &mut invariants);

    // Keep controller roots and template-owned elements synchronized with the document.
    for required_id in [
        "cielo-application",
        "locations-view",
        "municipality-search",
        "clear-search",
        "catalog-status",
        "catalog-retry",
        "saved-section",
        "empty-guidance",
        "empty-search",
        "saved-list",
        "results-section",
        "search-status",
        "results-list",
        "search-overflow-status",
        "source-update",
        "source-updated-at",
        "reorder-announcement",
        "forecast-view",
        "locations-button",
        "municipality-switcher",
        "municipality-title",
        "current-condition-icon",
        "current-condition-description",
        "current-temperature-announcement",
        "current-conditions-message",
        "hourly-forecast",
        "hourly-forecast-list",
        "municipality-row-template",
        "hourly-forecast-period-template",
    ] {
        assert!(
            invariants.ids.contains(required_id),
            "generated document is missing required ID {required_id}"
        );
    }
    for required_class in [
        "forecast-screen",
        "current-reading",
        "hourly-scroll",
        "hourly-hour",
        "hourly-condition-icon",
        "hourly-temperature",
        "municipality-name",
        "municipality-province",
        "open-button",
        "remove-button",
        "temperature",
        "condition-icon",
    ] {
        assert!(
            invariants.classes.contains(required_class),
            "generated document is missing required class {required_class}"
        );
    }

    assert_eq!(invariants.symbol_count, 19);
    for target in invariants.use_targets {
        assert!(
            invariants.ids.contains(&target),
            "icon reference does not resolve to a generated symbol: #{target}"
        );
    }
}

fn collect_document_invariants(handle: &Handle, invariants: &mut GeneratedDocumentInvariants) {
    if let NodeData::Element {
        name,
        attrs,
        template_contents,
        ..
    } = &handle.data
    {
        let attributes = attrs.borrow();
        let id = attribute_value(&attributes, "id");
        if let Some(id) = &id {
            assert!(
                invariants.ids.insert(id.clone()),
                "generated document contains duplicate ID {id}"
            );
        }
        if let Some(classes) = attribute_value(&attributes, "class") {
            invariants
                .classes
                .extend(classes.split_ascii_whitespace().map(str::to_owned));
        }

        match name.local.as_ref() {
            "symbol" => {
                invariants.symbol_count += 1;
                assert!(
                    id.as_deref()
                        .is_some_and(|value| value.starts_with("cielo-icon-")),
                    "generated symbol has an invalid ID"
                );
                assert!(
                    attribute_value(&attributes, "viewBox").is_some(),
                    "generated symbol is missing a viewBox"
                );
                assert!(
                    attribute_value(&attributes, "width").is_none()
                        && attribute_value(&attributes, "height").is_none(),
                    "generated symbol retains intrinsic dimensions"
                );
            }
            "use" => {
                if let Some(target) = attribute_value(&attributes, "href") {
                    let target = target
                        .strip_prefix('#')
                        .expect("icon reference should be document-local");
                    invariants.use_targets.push(target.to_owned());
                }
            }
            _ => {}
        }
        drop(attributes);

        if let Some(contents) = template_contents.borrow().as_ref() {
            collect_document_invariants(contents, invariants);
        }
    }

    for child in handle.children.borrow().iter() {
        collect_document_invariants(child, invariants);
    }
}

fn attribute_value(attributes: &[html5ever::Attribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.to_string())
}

fn sample_source_data() -> AemetWeatherData {
    AemetWeatherData {
        municipalities: HashMap::from([("35001".to_owned(), "Arco, El".to_owned())]),
        forecasts: vec![MunicipalityForecast {
            id: "35001".to_owned(),
            name: "Forecast fallback".to_owned(),
            province: "Las Palmas (Gran Canaria)".to_owned(),
            generated_at: "2026-07-25T08:00:00".to_owned(),
            hourly_forecasts: vec![HourlyForecast {
                date: "2026-07-25".to_owned(),
                hour: 10,
                temperature_celsius: 24,
                condition: WeatherCondition::CloudSun,
                description: "Intervalos nubosos".to_owned(),
            }],
        }],
    }
}
