use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, include_dir};
use reqwest::Url;
use serde::Serialize;
use tempfile::{Builder, TempDir};
use tracing::warn;

use crate::aemet::{AemetClient, AemetData, Forecast, Temperature, validate_municipality_id};

#[cfg(test)]
mod tests;

const AEMET_SOURCE_NAME: &str = "AEMET";
const AEMET_SOURCE_URL: &str = "https://opendata.aemet.es/";
const APP_CONFIG_FILENAME: &str = "assets/lib/config.js";
const APP_MARKER_CONTENT: &str = "cielo-output=app\ncielo-schema=1\n";
const DATA_MARKER_CONTENT: &str = "cielo-output=data\ncielo-schema=1\n";
const DATA_URL_MARKER: &str = r#""./data/" /* @cielo-data-url */"#;
const ICON_COMPONENT_FILENAME: &str = "assets/components/cielo-icon.js";
const ICON_GLYPHS_MARKER: &str = "/* @cielo-icon-glyphs */";
const ICONS_DIRECTORY: &str = "assets/icons";
const LEGACY_DATA_MARKER_CONTENT: &str = "cielo-schema=1\n";
const LEGACY_SITE_MARKER_CONTENT: &str = "cielo-output=site\ncielo-schema=1\n";
const MANAGED_MARKER: &str = ".cielo-generated";
const MUNICIPALITIES_FILENAME: &str = "municipalities.json";
const SCHEMA_VERSION: u8 = 1;
const SERVICE_WORKER_FILENAME: &str = "service-worker.js";
const TEMPERATURES_DIRECTORY: &str = "temperatures";

static APP_DIRECTORY: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/site");

/// Kind of generated artifact to publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputKind {
    App,
    Data,
}

impl OutputKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Data => "data",
        }
    }

    const fn marker_content(self) -> &'static str {
        match self {
            Self::App => APP_MARKER_CONTENT,
            Self::Data => DATA_MARKER_CONTENT,
        }
    }

    fn accepts_marker(self, marker: &str) -> bool {
        marker == self.marker_content()
            || self == Self::App && marker == LEGACY_SITE_MARKER_CONTENT
            || self == Self::Data && marker == LEGACY_DATA_MARKER_CONTENT
    }
}

/// Counts from a successfully published snapshot.
#[derive(Debug)]
pub(crate) struct DataBuildSummary {
    pub(crate) municipalities: usize,
    pub(crate) temperature_files: usize,
}

#[derive(Debug, Serialize)]
struct Source<'a> {
    name: &'a str,
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    generated_at: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct MunicipalitiesDocument<'a> {
    schema_version: u8,
    source: Source<'a>,
    municipalities: &'a [Municipality],
}

#[derive(Debug, Serialize)]
struct Municipality {
    id: String,
    name: String,
    province: String,
    timezone: Timezone,
}

#[derive(Debug, Serialize)]
struct TemperatureDocument<'a> {
    schema_version: u8,
    source: Source<'a>,
    municipality_id: &'a str,
    timezone: Timezone,
    temperatures: &'a [Temperature],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Timezone {
    #[serde(rename = "Africa/Ceuta")]
    AfricaCeuta,
    #[serde(rename = "Atlantic/Canary")]
    AtlanticCanary,
    #[serde(rename = "Europe/Madrid")]
    EuropeMadrid,
}

struct Snapshot {
    municipalities: Vec<Municipality>,
    forecasts: Vec<Forecast>,
}

/// Build and transactionally publish the static application shell.
pub(crate) fn build_app(output_dir: &Path, data_url: &str) -> Result<()> {
    let data_url = normalize_data_url(data_url)?;
    let output_dir = validate_output_directory(output_dir, OutputKind::App)?;

    // Render a complete artifact before replacing the currently published app.
    let staging = write_app_staging_directory(&output_dir, &data_url)?;
    publish_staging_directory(&staging, &output_dir, OutputKind::App)
}

/// Fetch and transactionally publish one complete weather-data snapshot.
pub(crate) async fn build_data(
    client: &AemetClient,
    output_dir: &Path,
) -> Result<DataBuildSummary> {
    // Reject unsafe destinations before making a potentially expensive request.
    let output_dir = validate_output_directory(output_dir, OutputKind::Data)?;
    let data = client.fetch().await?;
    let snapshot = build_snapshot(data)?;
    let summary = DataBuildSummary {
        municipalities: snapshot.municipalities.len(),
        temperature_files: snapshot.forecasts.len(),
    };

    // A complete sibling staging directory keeps readers away from partial data.
    let staging = write_data_staging_directory(&output_dir, &snapshot)?;
    publish_staging_directory(&staging, &output_dir, OutputKind::Data)?;

    Ok(summary)
}

fn build_snapshot(data: AemetData) -> Result<Snapshot> {
    let AemetData {
        municipalities: master_municipalities,
        mut forecasts,
    } = data;
    forecasts.sort_by(|left, right| left.id.cmp(&right.id));

    let mut forecast_ids = HashSet::with_capacity(forecasts.len());
    let mut municipalities = Vec::with_capacity(forecasts.len());
    let mut forecast_only_count = 0_usize;

    for forecast in &forecasts {
        validate_municipality_id(&forecast.id)?;
        if !forecast_ids.insert(forecast.id.as_str()) {
            bail!("duplicate forecast ID: {}", forecast.id);
        }

        let source_name = if let Some(master_name) = master_municipalities.get(&forecast.id) {
            master_name.trim()
        } else {
            forecast_only_count += 1;
            forecast.name.trim()
        };
        let name = normalize_municipality_name(source_name);
        if name.is_empty() {
            bail!("municipality {} has an empty name", forecast.id);
        }
        let province = normalize_province(&forecast.province);
        if province.is_empty() {
            bail!("municipality {} has an empty province", forecast.id);
        }

        municipalities.push(Municipality {
            id: forecast.id.clone(),
            name,
            province: province.to_owned(),
            timezone: timezone_for(&forecast.id),
        });
    }

    let master_only_count = master_municipalities
        .keys()
        .filter(|id| !forecast_ids.contains(id.as_str()))
        .count();
    if forecast_only_count > 0 || master_only_count > 0 {
        warn!(
            forecast_only = forecast_only_count,
            master_only = master_only_count,
            "AEMET municipality products contain different ID sets"
        );
    }

    Ok(Snapshot {
        municipalities,
        forecasts,
    })
}

fn write_app_staging_directory(output_dir: &Path, data_url: &str) -> Result<TempDir> {
    let staging = create_staging_directory(output_dir, OutputKind::App)?;
    write_app_files(staging.path(), data_url)?;
    Ok(staging)
}

fn write_data_staging_directory(output_dir: &Path, snapshot: &Snapshot) -> Result<TempDir> {
    let staging = create_staging_directory(output_dir, OutputKind::Data)?;
    write_data_files(staging.path(), snapshot)?;
    Ok(staging)
}

fn create_staging_directory(output_dir: &Path, output_kind: OutputKind) -> Result<TempDir> {
    let parent = output_dir
        .parent()
        .context("output directory does not have a parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create output parent {}", parent.display()))?;
    let staging = Builder::new()
        .prefix(".cielo-staging-")
        .tempdir_in(parent)
        .with_context(|| format!("failed to create staging directory in {}", parent.display()))?;

    write_managed_marker(staging.path(), output_kind)?;
    Ok(staging)
}

fn write_managed_marker(output_dir: &Path, output_kind: OutputKind) -> Result<()> {
    fs::write(
        output_dir.join(MANAGED_MARKER),
        output_kind.marker_content(),
    )
    .context("failed to write generated-directory marker")
}

fn write_app_files(output_dir: &Path, data_url: &str) -> Result<()> {
    // Keep the generated artifact independent from the source checkout.
    APP_DIRECTORY
        .extract(output_dir)
        .context("failed to write embedded application assets")?;

    // Complete deployment-specific configuration and generated asset catalogs.
    write_data_url_configuration(output_dir, data_url)?;
    write_icon_catalog(output_dir)
}

fn write_data_url_configuration(output_dir: &Path, data_url: &str) -> Result<()> {
    let encoded_data_url =
        serde_json::to_string(data_url).context("failed to encode application data URL")?;
    for filename in [APP_CONFIG_FILENAME, SERVICE_WORKER_FILENAME] {
        let path = output_dir.join(filename);
        let source = fs::read_to_string(&path)
            .with_context(|| format!("failed to read application asset {}", path.display()))?;
        if source.matches(DATA_URL_MARKER).count() != 1 {
            bail!(
                "application asset must contain exactly one data URL marker: {}",
                path.display()
            );
        }
        let configured = source.replacen(DATA_URL_MARKER, &encoded_data_url, 1);
        fs::write(&path, configured)
            .with_context(|| format!("failed to configure application asset {}", path.display()))?;
    }

    Ok(())
}

fn write_icon_catalog(output_dir: &Path) -> Result<()> {
    let icons_dir = output_dir.join(ICONS_DIRECTORY);
    let catalog = build_icon_catalog(&icons_dir)?;
    let component_path = output_dir.join(ICON_COMPONENT_FILENAME);
    let component = fs::read_to_string(&component_path)
        .with_context(|| format!("failed to read icon component {}", component_path.display()))?;
    let component = inject_icon_catalog(&component, &catalog)?;
    fs::write(&component_path, component).with_context(|| {
        format!(
            "failed to write icon component {}",
            component_path.display()
        )
    })
}

fn build_icon_catalog(icons_dir: &Path) -> Result<String> {
    let mut icon_paths = fs::read_dir(icons_dir)
        .with_context(|| format!("failed to read icon directory {}", icons_dir.display()))?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.path().extension().is_some_and(|value| value == "svg") => {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to inspect icon directory {}", icons_dir.display()))?;
    icon_paths.sort();
    if icon_paths.is_empty() {
        bail!(
            "icon directory does not contain SVG files: {}",
            icons_dir.display()
        );
    }

    // Stable ordering makes generated releases and reviews reproducible.
    let mut catalog = String::new();
    for icon_path in icon_paths {
        let icon_name = icon_path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("icon filename is not valid UTF-8")?;
        let source = fs::read_to_string(&icon_path)
            .with_context(|| format!("failed to read icon {}", icon_path.display()))?;
        let glyph = build_icon_glyph(icon_name, &source)?;
        let encoded_name =
            serde_json::to_string(icon_name).context("failed to encode icon name")?;
        let encoded_glyph = serde_json::to_string(glyph).context("failed to encode icon glyph")?;
        catalog.push_str("  [");
        catalog.push_str(&encoded_name);
        catalog.push_str(", ");
        catalog.push_str(&encoded_glyph);
        catalog.push_str("],\n");
    }

    Ok(catalog)
}

fn build_icon_glyph<'a>(icon_name: &str, source: &'a str) -> Result<&'a str> {
    if !is_valid_icon_name(icon_name) {
        bail!("invalid icon name: {icon_name}");
    }

    // Retain the source root attributes and body without copying glyphs by hand.
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

    let attributes = &source[root_start + "<svg".len()..root_open_end];
    if attributes.contains("id=") {
        bail!("icon SVG root must not define an ID");
    }

    Ok(&source[root_start..root_close_start + "</svg>".len()])
}

fn inject_icon_catalog(component: &str, catalog: &str) -> Result<String> {
    if component.matches(ICON_GLYPHS_MARKER).count() != 1 {
        bail!("icon component must contain exactly one glyph marker");
    }

    Ok(component.replacen(ICON_GLYPHS_MARKER, catalog, 1))
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

fn write_data_files(output_dir: &Path, snapshot: &Snapshot) -> Result<()> {
    let temperatures_dir = output_dir.join(TEMPERATURES_DIRECTORY);
    fs::create_dir(&temperatures_dir).context("failed to create temperatures directory")?;

    let municipalities_document = MunicipalitiesDocument {
        schema_version: SCHEMA_VERSION,
        source: Source {
            name: AEMET_SOURCE_NAME,
            url: AEMET_SOURCE_URL,
            generated_at: None,
        },
        municipalities: &snapshot.municipalities,
    };
    write_json(
        &output_dir.join(MUNICIPALITIES_FILENAME),
        &municipalities_document,
    )?;

    for forecast in &snapshot.forecasts {
        let document = TemperatureDocument {
            schema_version: SCHEMA_VERSION,
            source: Source {
                name: AEMET_SOURCE_NAME,
                url: AEMET_SOURCE_URL,
                generated_at: Some(&forecast.generated_at),
            },
            municipality_id: &forecast.id,
            timezone: timezone_for(&forecast.id),
            temperatures: &forecast.temperatures,
        };
        write_json(
            &temperatures_dir.join(format!("{}.json", forecast.id)),
            &document,
        )?;
    }

    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create generated file {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value)
        .with_context(|| format!("failed to serialize generated file {}", path.display()))?;
    writer
        .write_all(b"\n")
        .with_context(|| format!("failed to finish generated file {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("failed to flush generated file {}", path.display()))
}

fn publish_staging_directory(
    staging: &TempDir,
    output_dir: &Path,
    output_kind: OutputKind,
) -> Result<()> {
    // Revalidate after the download so a concurrent path change cannot bypass safety.
    validate_existing_output(output_dir, output_kind)?;
    if !output_dir.exists() {
        return fs::rename(staging.path(), output_dir).with_context(|| {
            format!(
                "failed to publish generated directory {}",
                output_dir.display()
            )
        });
    }

    let parent = output_dir
        .parent()
        .context("output directory does not have a parent")?;
    let backup = Builder::new()
        .prefix(".cielo-backup-")
        .tempdir_in(parent)
        .with_context(|| format!("failed to create backup directory in {}", parent.display()))?;
    let previous_snapshot = backup.path().join("snapshot");
    fs::rename(output_dir, &previous_snapshot).with_context(|| {
        format!(
            "failed to move previous generated directory {}",
            output_dir.display()
        )
    })?;

    if let Err(publish_error) = fs::rename(staging.path(), output_dir) {
        // Restore the prior valid snapshot when the final rename fails.
        let restore_result = fs::rename(&previous_snapshot, output_dir);
        return match restore_result {
            Ok(()) => Err(publish_error).with_context(|| {
                format!(
                    "failed to publish generated directory {}; previous snapshot restored",
                    output_dir.display()
                )
            }),
            Err(restore_error) => {
                let backup_path = backup.keep();
                bail!(
                    "failed to publish generated directory {} ({publish_error}); \
                     also failed to restore previous snapshot ({restore_error}); \
                     previous data remains at {}",
                    output_dir.display(),
                    backup_path.join("snapshot").display()
                )
            }
        };
    }

    if let Err(error) = backup.close() {
        warn!(%error, "failed to remove previous generated snapshot backup");
    }
    Ok(())
}

fn normalize_data_url(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("data URL cannot be empty");
    }
    if value.starts_with("//") {
        bail!("data URL must include an explicit http or https scheme");
    }
    if value.contains('\\') {
        bail!("data URL must use forward slashes");
    }

    // Resolve relative references against a fixed web base for uniform validation.
    let validation_base =
        Url::parse("https://cielo.invalid/app/").context("failed to prepare URL validation")?;
    let (url, is_absolute) = match Url::parse(value) {
        Ok(url) => (url, true),
        Err(_) => (
            validation_base
                .join(value)
                .with_context(|| format!("invalid data URL: {value}"))?,
            false,
        ),
    };
    if !matches!(url.scheme(), "http" | "https") {
        bail!("data URL must use http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("data URL must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("data URL must not contain a query or fragment");
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

fn validate_output_directory(path: &Path, output_kind: OutputKind) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("output directory cannot be empty");
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("output directory cannot contain '..'");
    }

    let current_dir = env::current_dir().context("failed to determine current directory")?;
    let absolute = if path.is_absolute() {
        clean_path(path)
    } else {
        clean_path(&current_dir.join(path))
    };
    if absolute == clean_path(&current_dir) || absolute.parent().is_none() {
        bail!("output directory must not be the current directory or filesystem root");
    }
    if absolute.file_name().is_none() {
        bail!("output directory must have a final path component");
    }

    validate_existing_output(&absolute, output_kind)?;
    Ok(absolute)
}

fn validate_existing_output(path: &Path, output_kind: OutputKind) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect output directory {}", path.display()));
        }
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "output directory must not be a symbolic link: {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        bail!("output path is not a directory: {}", path.display());
    }

    let is_empty = fs::read_dir(path)
        .with_context(|| format!("failed to read output directory {}", path.display()))?
        .next()
        .is_none();
    if is_empty {
        return Ok(());
    }

    let marker_path = path.join(MANAGED_MARKER);
    let marker_metadata = fs::symlink_metadata(&marker_path).with_context(|| {
        format!(
            "refusing to replace non-empty unmanaged directory {}",
            path.display()
        )
    })?;
    if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
        bail!("invalid generated-directory marker in {}", path.display());
    }
    let marker = fs::read_to_string(&marker_path)
        .with_context(|| format!("failed to read marker in {}", path.display()))?;
    if !output_kind.accepts_marker(&marker) {
        bail!(
            "generated directory {} is not managed as {} output",
            path.display(),
            output_kind.as_str()
        );
    }

    Ok(())
}

fn clean_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

fn normalize_province(province: &str) -> &str {
    let province = province.trim();
    let province = if let Some(base) = province
        .strip_suffix(')')
        .and_then(|value| value.rsplit_once(" (").map(|(base, _)| base.trim()))
    {
        base
    } else {
        province
    };

    province
        .rsplit_once('/')
        .map_or(province, |(_, spanish_name)| spanish_name.trim())
}

fn normalize_municipality_name(name: &str) -> String {
    name.trim()
        .split('/')
        .map(normalize_deferred_article)
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_deferred_article(name: &str) -> String {
    let name = name.trim();
    let Some((base, article)) = name.rsplit_once(',') else {
        return name.to_owned();
    };
    let base = base.trim();
    let article = article.trim();
    if base.is_empty() || !is_deferred_article(article) {
        return name.to_owned();
    }

    if article.ends_with('\'') || article.ends_with('’') {
        format!("{article}{base}")
    } else {
        format!("{article} {base}")
    }
}

fn is_deferred_article(value: &str) -> bool {
    matches!(
        value,
        "A" | "As"
            | "El"
            | "Els"
            | "Es"
            | "L'"
            | "L’"
            | "La"
            | "Las"
            | "Les"
            | "Los"
            | "O"
            | "Os"
            | "Sa"
            | "Ses"
            | "el"
            | "els"
            | "l'"
            | "l’"
            | "la"
            | "les"
    )
}

fn timezone_for(municipality_id: &str) -> Timezone {
    match municipality_id.get(..2) {
        Some("35" | "38") => Timezone::AtlanticCanary,
        Some("51" | "52") => Timezone::AfricaCeuta,
        _ => Timezone::EuropeMadrid,
    }
}
