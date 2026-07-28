use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
};

use crate::aemet::{AemetWeatherData, HourlyForecast, MunicipalityForecast, WeatherCondition};
use html5ever::{parse_document, tendril::TendrilSink};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use super::{
    application::{
        CONTENT_HASH_LENGTH, browser_asset_url, build_icon_symbol,
        build_script_assets_from_sources, content_hash, generate_application,
        normalize_weather_data_url, normalized_logical_path,
    },
    files::GeneratedFiles,
    publisher::{
        OutputKind, create_staging_directory, publish_application_directory,
        publish_staging_directory,
    },
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
fn normalizes_browser_asset_url_separators() {
    assert_eq!(
        normalized_logical_path(r"assets\scripts\dependency.js"),
        "assets/scripts/dependency.js"
    );
    assert_eq!(
        browser_asset_url(r"assets\styles\foundation.0123456789abcdef.css"),
        "./assets/styles/foundation.0123456789abcdef.css"
    );
}

#[test]
fn uses_a_short_sha256_prefix_for_content_hashes() {
    assert_eq!(CONTENT_HASH_LENGTH, 16);
    assert_eq!(content_hash(b""), "e3b0c44298fc1c14");
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
fn does_not_expose_invalid_weather_data_url() {
    let data_url = "https://[private-data-endpoint";

    let error =
        normalize_weather_data_url(data_url).expect_err("invalid data URL should be rejected");

    assert!(!format!("{error:#}").contains(data_url));
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
fn generated_files_count_entries_and_reject_unsafe_paths() {
    let mut files = GeneratedFiles::default();
    files
        .insert("z.js", b"z".to_vec())
        .expect("path should work");
    files
        .insert("a.css", b"a".to_vec())
        .expect("path should work");

    assert_eq!(files.file_count(), 2);
    assert!(files.insert(Path::new("../escape"), Vec::new()).is_err());
    assert!(files.insert(Path::new("/absolute"), Vec::new()).is_err());
}

#[test]
fn rewrites_and_hashes_javascript_dependency_graphs() {
    let sources = BTreeMap::from([
        (
            "assets/scripts/main.js",
            "import { value } from \"./dependency.js\";\nconsole.log(value);\n".to_owned(),
        ),
        (
            "assets/scripts/dependency.js",
            "export const value = 1;\n".to_owned(),
        ),
        (
            "assets/scripts/unrelated.js",
            "export const unrelated = true;\n".to_owned(),
        ),
    ]);
    let first =
        build_script_assets_from_sources(&sources).expect("script graph should build successfully");
    let dependency = first
        .get("assets/scripts/dependency.js")
        .expect("dependency should be built");
    let main = first
        .get("assets/scripts/main.js")
        .expect("entry module should be built");
    assert!(
        String::from_utf8(main.bytes.clone())
            .expect("rewritten module should be UTF-8")
            .contains(
                Path::new(&dependency.output_path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("dependency filename should be UTF-8")
            )
    );

    let mut changed_sources = sources;
    changed_sources.insert(
        "assets/scripts/dependency.js",
        "export const value = 2;\n".to_owned(),
    );
    let second = build_script_assets_from_sources(&changed_sources)
        .expect("changed script graph should build successfully");

    assert_ne!(
        first["assets/scripts/dependency.js"].output_path,
        second["assets/scripts/dependency.js"].output_path
    );
    assert_ne!(
        first["assets/scripts/main.js"].output_path,
        second["assets/scripts/main.js"].output_path
    );
    assert_eq!(
        first["assets/scripts/unrelated.js"].output_path,
        second["assets/scripts/unrelated.js"].output_path
    );
}

#[test]
fn rejects_invalid_javascript_dependency_graphs() {
    let missing = BTreeMap::from([(
        "assets/scripts/main.js",
        "import \"./missing.js\";\n".to_owned(),
    )]);
    let error = build_script_assets_from_sources(&missing)
        .expect_err("missing local dependency should fail");
    assert!(error.to_string().contains("missing local module"));

    let cycle = BTreeMap::from([
        (
            "assets/scripts/first.js",
            "import \"./second.js\";\n".to_owned(),
        ),
        (
            "assets/scripts/second.js",
            "import \"./first.js\";\n".to_owned(),
        ),
    ]);
    let error = build_script_assets_from_sources(&cycle).expect_err("dependency cycle should fail");
    assert!(error.to_string().contains("contains a cycle"));
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
fn incomplete_publication_leaves_precreated_empty_output_reusable() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    fs::create_dir(&output_directory).expect("empty output should be created");
    let (output_directory, staging) = create_staging_directory(&output_directory, OutputKind::App)
        .expect("staging directory should be created");
    let files = GeneratedFiles::default();

    let error = publish_application_directory(&staging, &output_directory, &files)
        .expect_err("application without index should be rejected");

    assert!(error.to_string().contains("does not contain index.html"));
    assert!(
        fs::read_dir(&output_directory)
            .expect("empty output should be readable")
            .next()
            .is_none()
    );
    generate_application(&output_directory, "../weather-data")
        .expect("empty output should remain reusable");
    assert!(output_directory.join("index.html").is_file());
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
        output_directory.join("catalog.json"),
        r#"{"generator":"cielo","provinces":[{"name":"Madrid","tz":"Europe/Madrid","municipalities":[]}]}"#,
    )
    .expect("catalog should be created");
    fs::write(output_directory.join("stale.json"), "{}").expect("stale file should be created");

    let (output_directory, staging) = create_staging_directory(&output_directory, OutputKind::Data)
        .expect("generated data output should be recognized");
    fs::write(
        staging.path().join("catalog.json"),
        r#"{"generator":"cielo","provinces":[{"name":"Madrid","tz":"Europe/Madrid","municipalities":[]}]}"#,
    )
    .expect("replacement catalog should be created");
    publish_staging_directory(&staging, &output_directory, OutputKind::Data)
        .expect("generated data output should be replaced");

    assert!(!output_directory.join("stale.json").exists());
    assert!(output_directory.join("catalog.json").is_file());
}

#[test]
fn accumulates_existing_generated_app_output() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    generate_application(&output_directory, "../weather-data-one")
        .expect("initial app should generate");
    let initial_paths = output_paths(&output_directory);
    let immutable_path = initial_paths
        .iter()
        .find(|path| path.as_str() != "index.html")
        .expect("immutable asset should exist")
        .clone();
    fs::write(output_directory.join("stale.js"), "stale").expect("stale file should be created");

    #[cfg(unix)]
    let immutable_inode = {
        use std::os::unix::fs::MetadataExt;

        fs::metadata(output_directory.join(&immutable_path))
            .expect("immutable asset metadata should read")
            .ino()
    };

    generate_application(&output_directory, "../weather-data-two")
        .expect("generated app should be replaced");

    assert!(output_directory.join("stale.js").is_file());
    for path in initial_paths {
        assert!(
            output_directory.join(&path).is_file(),
            "previous output should remain: {path}"
        );
    }
    let index =
        fs::read_to_string(output_directory.join("index.html")).expect("index should be readable");
    assert!(index.contains("data-weather-data-url=\"../weather-data-two/\""));
    assert!(!index.contains("data-weather-data-url=\"../weather-data-one/\""));

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        assert_eq!(
            fs::metadata(output_directory.join(&immutable_path))
                .expect("immutable asset metadata should read")
                .ino(),
            immutable_inode,
            "unchanged immutable asset should not be rewritten"
        );
    }
}

#[test]
fn conflicting_immutable_asset_does_not_replace_application_index() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let output_directory = temporary_root.path().join("application");
    generate_application(&output_directory, "../weather-data")
        .expect("initial app should generate");
    let original_index =
        fs::read(output_directory.join("index.html")).expect("index should be readable");
    let conflicting_path = output_paths(&output_directory)
        .into_iter()
        .find(|path| path != "index.html")
        .expect("immutable asset should exist");
    let (output_directory, staging) = create_staging_directory(&output_directory, OutputKind::App)
        .expect("staging directory should be created");
    let mut replacement = GeneratedFiles::default();
    replacement
        .insert(&conflicting_path, "conflicting bytes")
        .expect("conflicting asset should be generated");
    replacement
        .insert(
            "index.html",
            r#"<meta name="generator" content="cielo"><p>replacement</p>"#,
        )
        .expect("replacement index should be generated");
    replacement
        .write_to(staging.path())
        .expect("replacement should be staged");

    let error = publish_application_directory(&staging, &output_directory, &replacement)
        .expect_err("immutable conflict should fail");

    assert!(error.to_string().contains("conflicting content"));
    assert_eq!(
        fs::read(output_directory.join("index.html")).expect("index should remain readable"),
        original_index
    );
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
    assert!(paths.contains(&"index.html".to_owned()));
    for (directory, stem, extension) in [
        ("", "manifest", "webmanifest"),
        ("", "favicon", "svg"),
        ("assets/licenses", "lucide", "txt"),
        ("assets/styles", "design-tokens", "css"),
        ("assets/styles", "foundation", "css"),
        ("assets/styles", "locations", "css"),
        ("assets/styles", "forecast", "css"),
        ("assets/styles", "interactions", "css"),
        ("assets/scripts", "main", "js"),
        ("assets/scripts", "application-controller", "js"),
        ("assets/scripts", "locations-controller", "js"),
        ("assets/scripts", "forecast-controller", "js"),
        (
            "assets/scripts",
            "municipality-row-gesture-controller",
            "js",
        ),
        ("assets/scripts", "municipality-catalog", "js"),
        ("assets/scripts", "weather-data-client", "js"),
        ("assets/scripts", "forecast-store", "js"),
        ("assets/scripts", "preferences-store", "js"),
        ("assets/scripts", "dom", "js"),
        ("assets/icons", "apple-touch-icon", "png"),
        ("assets/icons", "application-192", "png"),
        ("assets/icons", "application-512", "png"),
        ("assets/icons", "application-maskable-512", "png"),
        ("assets/fonts", "manrope-latin-wght", "woff2"),
        ("assets/fonts", "manrope-latin-ext-wght", "woff2"),
        ("assets/licenses", "manrope", "txt"),
    ] {
        find_hashed_path(&paths, directory, stem, extension);
    }
    for path in paths.iter().filter(|path| path.as_str() != "index.html") {
        assert_content_hash(&output_directory, path);
    }
    assert!(!paths.contains(&"service-worker.js".to_owned()));
    assert!(paths.iter().all(|path| !path.starts_with('.')));
    assert!(paths.iter().all(|path| !path.contains(".DS_Store")));
    assert!(
        paths
            .iter()
            .all(|path| !has_extension(path, "svg") || path.starts_with("favicon."))
    );
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
    assert_generated_fonts(&output_directory, &paths, &index);

    for path in paths.iter().filter(|path| has_extension(path, "css")) {
        assert!(index.contains(&format!("./{path}")));
        let css =
            fs::read_to_string(output_directory.join(path)).expect("stylesheet should be readable");
        StyleSheet::parse(&css, ParserOptions::default())
            .unwrap_or_else(|error| panic!("invalid CSS in {path}: {error:?}"));
    }

    for path in paths.iter().filter(|path| has_extension(path, "js")) {
        let script =
            fs::read_to_string(output_directory.join(path)).expect("script should be readable");
        for specifier in local_javascript_specifiers(&script) {
            let referenced_path = Path::new(path)
                .parent()
                .expect("script should have a parent")
                .join(
                    specifier
                        .strip_prefix("./")
                        .expect("local specifier should be relative"),
                )
                .to_string_lossy()
                .replace('\\', "/");
            assert!(
                paths.contains(&referenced_path),
                "{path} references missing {referenced_path}"
            );
        }
        assert!(!script.contains("innerHTML"));
        assert!(!script.contains("<style>"));
        assert!(!script.contains("customElements"));
    }

    assert_application_manifest(&output_directory, &paths, &index);
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

    assert_eq!(
        snapshot.forecasts[0].hourly_forecasts[0].temperature_celsius,
        24
    );
    assert_eq!(
        snapshot.forecasts[0].hourly_forecasts[0].condition,
        WeatherCondition::CloudSun
    );
}

#[test]
fn writes_readable_weather_data_files() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let snapshot = build_snapshot(sample_source_data()).expect("snapshot should build");
    let mut statistics = WeatherDataStatistics::default();

    let bundle_files = write_weather_data_files(temporary_root.path(), &snapshot, &mut statistics)
        .expect("weather-data files should write");

    assert_eq!(
        output_paths(temporary_root.path()),
        ["catalog.json", "forecasts/35/000.json"]
    );
    assert_eq!(bundle_files, 1);
    let catalog: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary_root.path().join("catalog.json")).expect("catalog should be readable"),
    )
    .expect("catalog should decode");
    let bundle: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary_root.path().join("forecasts/35/000.json"))
            .expect("forecast bundle should be readable"),
    )
    .expect("forecast bundle should decode");
    let forecast = &bundle["forecasts"]["35001"];

    assert_eq!(catalog["generator"], "cielo");
    assert_eq!(catalog["updated_at"], "2026-07-25T08:00:00");
    assert_eq!(catalog["provinces"][0]["name"], "Las Palmas");
    assert_eq!(catalog["provinces"][0]["tz"], "Atlantic/Canary");
    assert_eq!(catalog["provinces"][0]["municipalities"][0]["id"], "35001");
    assert_eq!(forecast[0]["date"], "2026-07-25");
    assert_eq!(forecast[0]["hours"][0]["temp_c"], 24);
    assert_eq!(forecast[0]["hours"][0]["state"], "cloud-sun");
    assert_eq!(forecast[0]["hours"][0]["desc"], "Intervalos nubosos");

    // Generated JSON has no formatting-only whitespace.
    for path in [
        temporary_root.path().join("catalog.json"),
        temporary_root.path().join("forecasts/35/000.json"),
    ] {
        let bytes = fs::read(path).expect("weather-data file should be readable");
        assert_eq!(bytes.last(), Some(&b'}'));
        assert!(!bytes.contains(&b'\n'));
        assert!(!bytes.contains(&b'\t'));
    }
}

#[test]
fn groups_forecasts_into_stable_twenty_id_ranges() {
    let temporary_root = tempfile::tempdir().expect("temporary root should be created");
    let mut source_data = sample_source_data();
    source_data.municipalities.clear();
    source_data.forecasts.clear();
    for id in ["35000", "35019", "35020", "36000"] {
        source_data
            .municipalities
            .insert(id.to_owned(), format!("Municipality {id}"));
        source_data.forecasts.push(sample_forecast(id));
    }
    let snapshot = build_snapshot(source_data).expect("snapshot should build");
    let mut statistics = WeatherDataStatistics::default();

    let bundle_files = write_weather_data_files(temporary_root.path(), &snapshot, &mut statistics)
        .expect("weather-data files should write");

    assert_eq!(bundle_files, 3);
    assert_eq!(
        output_paths(temporary_root.path()),
        [
            "catalog.json",
            "forecasts/35/000.json",
            "forecasts/35/020.json",
            "forecasts/36/000.json",
        ]
    );
    let first_bundle: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary_root.path().join("forecasts/35/000.json"))
            .expect("first bundle should be readable"),
    )
    .expect("first bundle should decode");
    let forecasts = first_bundle["forecasts"]
        .as_object()
        .expect("bundle forecasts should be an object");
    assert_eq!(
        forecasts.keys().map(String::as_str).collect::<Vec<_>>(),
        ["35000", "35019"]
    );
    assert!(forecasts["35019"].get("municipality_id").is_none());
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

fn find_hashed_path<'a>(
    paths: &'a [String],
    directory: &str,
    stem: &str,
    extension: &str,
) -> &'a str {
    paths
        .iter()
        .find(|path| {
            let path = Path::new(path);
            let parent = path
                .parent()
                .and_then(Path::to_str)
                .expect("generated path parent should be UTF-8");
            let Some(hashed_stem) = path.file_stem().and_then(|value| value.to_str()) else {
                return false;
            };
            let Some(hash) = hashed_stem
                .strip_prefix(stem)
                .and_then(|value| value.strip_prefix('.'))
            else {
                return false;
            };
            parent == directory
                && path
                    .extension()
                    .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                && hash.len() == CONTENT_HASH_LENGTH
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .map_or_else(
            || panic!("missing hashed asset {directory}/{stem}.*.{extension}"),
            String::as_str,
        )
}

fn assert_content_hash(root: &Path, relative_path: &str) {
    let path = Path::new(relative_path);
    let hash = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.rsplit_once('.'))
        .map(|(_, hash)| hash)
        .expect("hashed filename should contain its digest");
    let bytes = fs::read(root.join(path)).expect("hashed asset should be readable");
    let expected = content_hash(&bytes);
    assert_eq!(hash, expected, "content hash mismatch for {relative_path}");
}

fn local_javascript_specifiers(source: &str) -> Vec<String> {
    let mut specifiers = Vec::new();
    for (quote_position, quote) in source.char_indices() {
        if !matches!(quote, '"' | '\'') {
            continue;
        }
        let remaining = &source[quote_position + quote.len_utf8()..];
        if !remaining.starts_with("./") {
            continue;
        }
        let Some(value_end) = remaining.find(quote) else {
            continue;
        };
        let specifier = &remaining[..value_end];
        if Path::new(specifier)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
        {
            specifiers.push(specifier.to_owned());
        }
    }
    specifiers
}

fn assert_generated_fonts(root: &Path, paths: &[String], index: &str) {
    for declaration in [
        "font-family: \"Manrope\"",
        "font-display: optional",
        "font-weight: 200 800",
        "rel=\"preload\"",
        "as=\"font\"",
        "type=\"font/woff2\"",
        "crossorigin",
    ] {
        assert!(index.contains(declaration));
    }

    for path in [
        find_hashed_path(paths, "assets/fonts", "manrope-latin-wght", "woff2"),
        find_hashed_path(paths, "assets/fonts", "manrope-latin-ext-wght", "woff2"),
    ] {
        assert!(index.contains(&format!("./{path}")));
        let font = fs::read(root.join(path)).expect("font should be readable");
        assert_eq!(font.get(..4), Some(b"wOF2".as_slice()));
    }
}

fn assert_application_manifest(root: &Path, paths: &[String], index: &str) {
    let manifest_path = find_hashed_path(paths, "", "manifest", "webmanifest");
    assert!(index.contains(&format!("./{manifest_path}")));
    let manifest_bytes = fs::read(root.join(manifest_path)).expect("manifest should be readable");
    assert_eq!(manifest_bytes.last(), Some(&b'}'));
    assert!(!manifest_bytes.contains(&b'\n'));
    assert!(!manifest_bytes.contains(&b'\t'));
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest should decode");
    assert_eq!(manifest["id"], "./");
    assert_eq!(manifest["start_url"], "./");
    for icon in manifest["icons"]
        .as_array()
        .expect("manifest icons should be an array")
    {
        let path = icon["src"]
            .as_str()
            .and_then(|value| value.strip_prefix("./"))
            .expect("manifest icon should use a relative URL");
        assert!(
            paths.contains(&path.to_owned()),
            "missing manifest icon {path}"
        );
    }

    for path in [
        find_hashed_path(paths, "", "favicon", "svg"),
        find_hashed_path(paths, "assets/icons", "apple-touch-icon", "png"),
        find_hashed_path(paths, "assets/scripts", "main", "js"),
        find_hashed_path(paths, "assets/licenses", "lucide", "txt"),
        find_hashed_path(paths, "assets/licenses", "manrope", "txt"),
    ] {
        assert!(index.contains(&format!("./{path}")));
    }
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
        forecasts: vec![sample_forecast("35001")],
    }
}

fn sample_forecast(id: &str) -> MunicipalityForecast {
    let province = match id.get(..2) {
        Some("36") => "Pontevedra",
        _ => "Las Palmas (Gran Canaria)",
    };

    MunicipalityForecast {
        id: id.to_owned(),
        name: "Forecast fallback".to_owned(),
        province: province.to_owned(),
        generated_at: "2026-07-25T08:00:00".to_owned(),
        hourly_forecasts: vec![HourlyForecast {
            date: "2026-07-25".to_owned(),
            hour: 10,
            temperature_celsius: 24,
            condition: WeatherCondition::CloudSun,
            description: "Intervalos nubosos".to_owned(),
        }],
    }
}
