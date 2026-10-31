use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use html5ever::{parse_document, tendril::TendrilSink};
use lightningcss::{
    declaration::DeclarationBlock,
    rules::CssRule,
    stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
};
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

fn build_app(root: &Path) -> (std::path::PathBuf, Vec<String>) {
    let output = root.join("app");
    generate_application(&output, "../data").expect("app should build");
    let mut paths = Vec::new();
    let mut directories = vec![output.clone()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).expect("directory should be readable") {
            let entry = entry.expect("entry");
            if entry.file_type().expect("type").is_dir() {
                directories.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(&output)
                    .expect("relative")
                    .to_owned();
                paths.push(normalized_logical_path(&relative.to_string_lossy()));
            }
        }
    }
    paths.sort();
    (output, paths)
}

#[derive(Default)]
struct DocumentFacts {
    ids: BTreeSet<String>,
    meta: BTreeMap<String, Vec<String>>,
    stylesheets: Vec<String>,
    scripts: Vec<String>,
    data_url: Option<String>,
}

fn collect(handle: &Handle, facts: &mut DocumentFacts) {
    if let NodeData::Element { name, attrs, .. } = &handle.data {
        let attrs = attrs.borrow();
        let value = |key: &str| {
            attrs
                .iter()
                .find(|attribute| attribute.name.local.as_ref() == key)
                .map(|attribute| attribute.value.to_string())
        };
        if let Some(id) = value("id") {
            assert!(facts.ids.insert(id.clone()), "duplicate ID {id}");
        }
        match name.local.as_ref() {
            "meta" => {
                if let (Some(name), Some(content)) = (value("name"), value("content")) {
                    facts.meta.entry(name).or_default().push(content);
                }
            }
            "link" if value("rel").as_deref() == Some("stylesheet") => {
                facts
                    .stylesheets
                    .push(value("href").expect("stylesheet href"));
            }
            "script" => facts.scripts.extend(value("src")),
            "main" => facts.data_url = value("data-weather-data-url"),
            _ => {}
        }
    }
    for child in handle.children.borrow().iter() {
        collect(child, facts);
    }
}

#[test]
fn weather_icons_keep_their_colours() {
    let root = tempfile::tempdir().expect("temporary root");
    let (output, _) = build_app(root.path());
    let index = fs::read_to_string(output.join("index.html")).expect("index");
    // Weather icons keep their sun and rain colours through the theme
    // variables, on the shapes themselves (colours on the root are dropped).
    for (icon, variable) in [
        ("sun", "--icon-sun"),
        ("cloud-sun", "--icon-sun"),
        ("haze", "--icon-sun"),
        ("cloud-lightning", "--icon-sun"),
        ("cloud-rain", "--icon-accent"),
        ("cloud-drizzle", "--icon-accent"),
    ] {
        let start = index
            .find(&format!("id=\"cielo-icon-{icon}\""))
            .unwrap_or_else(|| panic!("sprite should contain {icon}"));
        let symbol = &index[start..start + index[start..].find("</symbol>").expect("symbol end")];
        assert!(symbol.contains(variable), "{icon} should use {variable}");
    }
}

#[test]
fn generates_a_complete_app_shell() {
    let root = tempfile::tempdir().expect("temporary root");
    let (output, paths) = build_app(root.path());
    let index = fs::read_to_string(output.join("index.html")).expect("index");
    let dom = parse_document(RcDom::default(), html5ever::ParseOpts::default()).one(index.as_str());
    let mut facts = DocumentFacts::default();
    collect(&dom.document, &mut facts);

    for id in [
        "cielo-application",
        "locations-view",
        "forecast-view",
        "beach-view",
        "sheet",
    ] {
        assert!(facts.ids.contains(id), "index should contain #{id}");
    }
    assert!(facts.ids.contains("cielo-icon-sun"));
    assert!(facts.ids.contains("cielo-icon-triangle-alert"));
    assert_eq!(facts.meta["text-scale"], ["scale"]);
    assert_eq!(facts.meta["theme-color"], ["#221f22"]);
    assert_eq!(facts.meta["generator"], ["cielo"]);
    assert_eq!(facts.data_url.as_deref(), Some("../data/"));
    assert!(
        !index.contains("stroke=\"#"),
        "icons must follow the text colour"
    );

    // Stylesheets keep their numbered order and every referenced file exists.
    let names = facts
        .stylesheets
        .iter()
        .map(|href| {
            href.rsplit('/')
                .next()
                .unwrap_or_default()
                .split('.')
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(names.len() >= 5);
    for href in facts.stylesheets.iter().chain(&facts.scripts) {
        assert!(
            output.join(href.trim_start_matches("./")).is_file(),
            "{href} should exist"
        );
    }

    // Every hashed file is named after its content.
    for path in &paths {
        let file_name = path.rsplit('/').next().unwrap_or_default();
        let parts = file_name.split('.').collect::<Vec<_>>();
        if parts.len() == 3 && parts[1].len() == CONTENT_HASH_LENGTH {
            let bytes = fs::read(output.join(path)).expect("asset");
            assert_eq!(parts[1], content_hash(&bytes), "{path} hash");
        }
    }

    // The service worker precaches exactly the shell and hashed assets.
    let worker = fs::read_to_string(output.join("sw.js")).expect("service worker");
    assert!(!worker.contains("__CIELO_"), "placeholders are replaced");
    let start = worker.find("const PRECACHE = ").expect("precache") + "const PRECACHE = ".len();
    let end = start + worker[start..].find(";\n").expect("end of precache");
    let precache = serde_json::from_str::<Vec<String>>(&worker[start..end]).expect("precache JSON");
    assert_eq!(precache[0], "./");
    for entry in &precache[1..] {
        assert!(
            output.join(entry.trim_start_matches("./")).is_file(),
            "{entry} should exist"
        );
    }
    let expected = paths
        .iter()
        .filter(|path| {
            !matches!(path.as_str(), "index.html" | "sw.js")
                && !path.starts_with("assets/licenses/")
        })
        .count();
    assert_eq!(precache.len() - 1, expected);

    // Module imports point at files that exist next to the importer.
    for path in paths
        .iter()
        .filter(|path| has_extension(path, "js") && path.starts_with("assets/scripts/"))
    {
        let source = fs::read_to_string(output.join(path)).expect("script");
        for line in source.lines().filter(|line| line.contains(" from \"./")) {
            let specifier = line
                .split("from \"./")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("specifier");
            assert!(
                output.join("assets/scripts").join(specifier).is_file(),
                "{path} imports missing {specifier}"
            );
        }
    }
}

#[test]
fn stylesheets_parse_and_scale_with_text_size() {
    let root = tempfile::tempdir().expect("temporary root");
    let (output, paths) = build_app(root.path());
    for path in paths.iter().filter(|path| has_extension(path, "css")) {
        let source = fs::read_to_string(output.join(path)).expect("stylesheet");
        let sheet = StyleSheet::parse(&source, ParserOptions::default())
            .unwrap_or_else(|error| panic!("{path} should parse: {error}"));
        let mut problems = Vec::new();
        check_rules(&sheet.rules.0, &mut problems);
        assert!(
            problems.is_empty(),
            "{path}: sizes must scale with text, found {problems:?}"
        );
        assert!(
            !source.contains("html {\n  font-size")
                && !source.contains("font: -apple-system-body;\n      font-size"),
            "{path} must not fix the root font size"
        );
    }
}

/// Collect declarations that size text or boxes in pixels.
fn check_rules(rules: &[CssRule], problems: &mut Vec<String>) {
    for rule in rules {
        match rule {
            CssRule::Style(style) => {
                check_declarations(&style.declarations, problems);
                check_rules(&style.rules.0, problems);
            }
            CssRule::Media(media) => check_rules(&media.rules.0, problems),
            CssRule::Supports(supports) => check_rules(&supports.rules.0, problems),
            CssRule::Container(container) => check_rules(&container.rules.0, problems),
            _ => {}
        }
    }
}

fn check_declarations(declarations: &DeclarationBlock, problems: &mut Vec<String>) {
    for (property, _important) in declarations.iter() {
        let name = property.property_id().name().to_owned();
        let watched = matches!(
            name.as_str(),
            "font-size"
                | "line-height"
                | "height"
                | "block-size"
                | "min-height"
                | "min-block-size"
                | "width"
                | "min-width"
        );
        if !watched {
            continue;
        }
        let value = property
            .value_to_css_string(PrinterOptions::default())
            .unwrap_or_default();
        // One-pixel helpers (visually hidden text) are allowed.
        if value.contains("px") && value != "1px" {
            problems.push(format!("{name}: {value}"));
        }
    }
}

#[test]
fn app_generation_is_deterministic() {
    let first = tempfile::tempdir().expect("temporary root");
    let second = tempfile::tempdir().expect("temporary root");
    let (first_output, first_paths) = build_app(first.path());
    let (second_output, second_paths) = build_app(second.path());
    assert_eq!(first_paths, second_paths);
    for file in ["index.html", "sw.js"] {
        assert_eq!(
            fs::read(first_output.join(file)).expect("file"),
            fs::read(second_output.join(file)).expect("file"),
        );
    }
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
