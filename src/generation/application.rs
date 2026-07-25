use std::path::Path;

use anyhow::{Context, Result, bail};
use askama::Template;
use include_dir::{Dir, include_dir};
use reqwest::Url;
use serde::Serialize;

use super::{
    GENERATOR_IDENTITY,
    files::GeneratedFiles,
    publisher::{OutputKind, create_staging_directory, publish_staging_directory},
};

const APPLICATION_NAME: &str = "Cielo";
const APPLICATION_DESCRIPTION: &str = "Municipios con predicción meteorológica de AEMET";
const APPLICATION_THEME_COLOR: &str = "#285b78";

const STYLE_ASSETS: [(&str, &str); 5] = [
    (
        "styles/design-tokens.css",
        "assets/styles/design-tokens.css",
    ),
    ("styles/foundation.css", "assets/styles/foundation.css"),
    ("styles/locations.css", "assets/styles/locations.css"),
    ("styles/forecast.css", "assets/styles/forecast.css"),
    ("styles/interactions.css", "assets/styles/interactions.css"),
];
const SCRIPT_ASSETS: [(&str, &str); 10] = [
    ("scripts/main.js", "assets/scripts/main.js"),
    (
        "scripts/application-controller.js",
        "assets/scripts/application-controller.js",
    ),
    (
        "scripts/locations-controller.js",
        "assets/scripts/locations-controller.js",
    ),
    (
        "scripts/forecast-controller.js",
        "assets/scripts/forecast-controller.js",
    ),
    (
        "scripts/municipality-row-gesture-controller.js",
        "assets/scripts/municipality-row-gesture-controller.js",
    ),
    (
        "scripts/municipality-catalog.js",
        "assets/scripts/municipality-catalog.js",
    ),
    (
        "scripts/weather-data-client.js",
        "assets/scripts/weather-data-client.js",
    ),
    (
        "scripts/forecast-store.js",
        "assets/scripts/forecast-store.js",
    ),
    (
        "scripts/preferences-store.js",
        "assets/scripts/preferences-store.js",
    ),
    ("scripts/dom.js", "assets/scripts/dom.js"),
];
const APPLICATION_ICON_ASSETS: [(&str, &str); 4] = [
    (
        "application-icons/apple-touch-icon.png",
        "assets/icons/apple-touch-icon.png",
    ),
    (
        "application-icons/application-192.png",
        "assets/icons/application-192.png",
    ),
    (
        "application-icons/application-512.png",
        "assets/icons/application-512.png",
    ),
    (
        "application-icons/application-maskable-512.png",
        "assets/icons/application-maskable-512.png",
    ),
];

static WEB_SOURCE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/web");

#[derive(Debug)]
pub(crate) struct ApplicationGenerationSummary {
    pub(crate) files: usize,
    pub(crate) bytes: usize,
}

#[derive(Template)]
#[template(path = "index.html")]
struct ApplicationDocumentTemplate<'a> {
    application_name: &'a str,
    description: &'a str,
    generator_identity: &'a str,
    theme_color: &'a str,
    weather_data_url: &'a str,
    icon_symbols: &'a str,
}

#[derive(Serialize)]
struct WebApplicationManifest<'a> {
    id: &'a str,
    name: &'a str,
    short_name: &'a str,
    description: &'a str,
    lang: &'a str,
    dir: &'a str,
    start_url: &'a str,
    scope: &'a str,
    display: &'a str,
    orientation: &'a str,
    background_color: &'a str,
    theme_color: &'a str,
    icons: Vec<ManifestIcon<'a>>,
}

#[derive(Serialize)]
struct ManifestIcon<'a> {
    src: &'a str,
    sizes: &'a str,
    r#type: &'a str,
    purpose: &'a str,
}

pub(crate) fn generate_application(
    output_directory: &Path,
    weather_data_url: &str,
) -> Result<ApplicationGenerationSummary> {
    let weather_data_url = normalize_weather_data_url(weather_data_url)?;
    let (output_directory, staging) = create_staging_directory(output_directory, OutputKind::App)?;

    // Build the complete payload before publishing any application files.
    let files = build_application_files(&weather_data_url)?;
    files.write_to(staging.path())?;

    let summary = ApplicationGenerationSummary {
        files: files.file_count(),
        bytes: files.total_bytes(),
    };
    publish_staging_directory(&staging, &output_directory, OutputKind::App)?;
    Ok(summary)
}

fn build_application_files(weather_data_url: &str) -> Result<GeneratedFiles> {
    let mut files = GeneratedFiles::default();
    let icon_symbols = build_icon_symbols()?;
    let index = ApplicationDocumentTemplate {
        application_name: APPLICATION_NAME,
        description: APPLICATION_DESCRIPTION,
        generator_identity: GENERATOR_IDENTITY,
        theme_color: APPLICATION_THEME_COLOR,
        weather_data_url,
        icon_symbols: &icon_symbols,
    }
    .render()
    .context("failed to render application document")?;
    files.insert("index.html", terminated_text(index))?;
    files.insert("favicon.svg", source_file("favicon.svg")?)?;
    files.insert("assets/licenses/lucide.txt", source_file("icons/LICENSE")?)?;

    for (source, destination) in STYLE_ASSETS
        .into_iter()
        .chain(SCRIPT_ASSETS)
        .chain(APPLICATION_ICON_ASSETS)
    {
        files.insert(destination, source_file(source)?)?;
    }

    let manifest = WebApplicationManifest {
        id: "./",
        name: APPLICATION_NAME,
        short_name: APPLICATION_NAME,
        description: "Aplicación del tiempo",
        lang: "es",
        dir: "ltr",
        start_url: "./",
        scope: "./",
        display: "standalone",
        orientation: "any",
        background_color: APPLICATION_THEME_COLOR,
        theme_color: APPLICATION_THEME_COLOR,
        icons: vec![
            ManifestIcon {
                src: "./assets/icons/application-192.png",
                sizes: "192x192",
                r#type: "image/png",
                purpose: "any",
            },
            ManifestIcon {
                src: "./assets/icons/application-512.png",
                sizes: "512x512",
                r#type: "image/png",
                purpose: "any",
            },
            ManifestIcon {
                src: "./assets/icons/application-maskable-512.png",
                sizes: "512x512",
                r#type: "image/png",
                purpose: "maskable",
            },
        ],
    };
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest).context("failed to encode web manifest")?;
    manifest_bytes.push(b'\n');
    files.insert("manifest.webmanifest", manifest_bytes)?;
    Ok(files)
}

fn build_icon_symbols() -> Result<String> {
    let mut icon_paths = ["icons/interface", "icons/weather"]
        .into_iter()
        .flat_map(|directory| {
            WEB_SOURCE
                .get_dir(directory)
                .into_iter()
                .flat_map(Dir::files)
                .filter(|file| file.path().extension().is_some_and(|value| value == "svg"))
                .map(|file| file.path().to_owned())
        })
        .collect::<Vec<_>>();
    icon_paths.sort();
    if icon_paths.is_empty() {
        bail!("web source does not contain interface or weather icons");
    }

    let mut symbols = String::new();
    for path in icon_paths {
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("icon filename is not valid UTF-8")?;
        let source = WEB_SOURCE
            .get_file(&path)
            .and_then(include_dir::File::contents_utf8)
            .with_context(|| format!("icon is not valid UTF-8: {}", path.display()))?;
        let symbol = build_icon_symbol(name, source)?;
        for line in symbol.lines() {
            symbols.push_str("      ");
            symbols.push_str(line);
            symbols.push('\n');
        }
    }
    Ok(symbols)
}

pub(super) fn build_icon_symbol(name: &str, source: &str) -> Result<String> {
    if !is_valid_icon_name(name) {
        bail!("invalid icon name: {name}");
    }
    let root_start = source
        .find("<svg")
        .context("icon does not contain an SVG root")?;
    let root_open_end = source[root_start..]
        .find('>')
        .map(|offset| root_start + offset)
        .context("icon SVG root is not closed")?;
    let root_close_start = source
        .rfind("</svg>")
        .context("icon does not close its SVG root")?;
    if root_close_start <= root_open_end {
        bail!("icon SVG root closes before its content");
    }
    if !source[root_close_start + "</svg>".len()..]
        .trim()
        .is_empty()
    {
        bail!("icon contains content after its SVG root");
    }

    // Keep the glyph metadata while letting each rendered icon own its dimensions.
    let attributes = scalable_symbol_attributes(&source[root_start + "<svg".len()..root_open_end])?;
    let body = &source[root_open_end + 1..root_close_start];
    Ok(format!(
        "<symbol id=\"cielo-icon-{name}\"{attributes}>{body}</symbol>"
    ))
}

fn scalable_symbol_attributes(attributes: &str) -> Result<String> {
    let mut remaining = attributes;
    let mut normalized = String::new();

    // Parse the controlled SVG root without carrying intrinsic dimensions into the sprite.
    loop {
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        let name_end = remaining
            .find(|character: char| character.is_ascii_whitespace() || character == '=')
            .unwrap_or(remaining.len());
        if name_end == 0 {
            bail!("icon SVG root contains an invalid attribute");
        }
        let name = &remaining[..name_end];
        remaining = remaining[name_end..].trim_start();
        remaining = remaining
            .strip_prefix('=')
            .with_context(|| format!("icon SVG attribute is missing '=': {name}"))?
            .trim_start();

        let quote = remaining
            .chars()
            .next()
            .context("icon SVG attribute is missing a value")?;
        if !matches!(quote, '"' | '\'') {
            bail!("icon SVG attribute value must be quoted: {name}");
        }
        let value_start = quote.len_utf8();
        let value_end = remaining[value_start..]
            .find(quote)
            .map(|offset| value_start + offset)
            .with_context(|| format!("icon SVG attribute value is not closed: {name}"))?;
        let value = &remaining[value_start..value_end];
        remaining = &remaining[value_end + quote.len_utf8()..];

        if name == "id" {
            bail!("icon SVG root must not define an ID");
        }
        if matches!(name, "width" | "height") {
            continue;
        }

        normalized.push(' ');
        normalized.push_str(name);
        normalized.push('=');
        normalized.push(quote);
        normalized.push_str(value);
        normalized.push(quote);
    }

    Ok(normalized)
}

fn source_file(path: &str) -> Result<Vec<u8>> {
    WEB_SOURCE
        .get_file(path)
        .map(|file| file.contents().to_vec())
        .with_context(|| format!("required web source is missing: {path}"))
}

pub(super) fn normalize_weather_data_url(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("weather-data URL cannot be empty");
    }
    if value.starts_with("//") {
        bail!("weather-data URL must include an explicit http or https scheme");
    }
    if value.contains('\\') {
        bail!("weather-data URL must use forward slashes");
    }

    let validation_base = Url::parse("https://cielo.invalid/application/")
        .context("failed to prepare URL validation")?;
    let (url, is_absolute) = match Url::parse(value) {
        Ok(url) => (url, true),
        Err(_) => (
            validation_base
                .join(value)
                .with_context(|| format!("invalid weather-data URL: {value}"))?,
            false,
        ),
    };
    if !matches!(url.scheme(), "http" | "https") {
        bail!("weather-data URL must use http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("weather-data URL must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("weather-data URL must not contain a query or fragment");
    }

    let mut normalized = if is_absolute {
        url.to_string()
    } else {
        value.to_owned()
    };
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    Ok(normalized)
}

fn is_valid_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        })
}

fn terminated_text(mut value: String) -> Vec<u8> {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value.into_bytes()
}
