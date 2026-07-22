use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, include_dir};
use serde::Serialize;
use tempfile::{Builder, TempDir};
use tracing::warn;

use crate::aemet::{AemetClient, AemetData, Forecast, Temperature, validate_municipality_id};

#[cfg(test)]
mod tests;

const AEMET_SOURCE_NAME: &str = "AEMET";
const AEMET_SOURCE_URL: &str = "https://opendata.aemet.es/";
const DATA_DIRECTORY: &str = "data";
const DATA_MARKER_CONTENT: &str = "cielo-output=data\ncielo-schema=1\n";
const ICONS_DIRECTORY: &str = "assets/icons";
const ICON_SPRITE_FILENAME: &str = "assets/icons.svg";
const LEGACY_DATA_MARKER_CONTENT: &str = "cielo-schema=1\n";
const MANAGED_MARKER: &str = ".cielo-generated";
const MUNICIPALITIES_FILENAME: &str = "municipalities.json";
const SCHEMA_VERSION: u8 = 1;
const SITE_MARKER_CONTENT: &str = "cielo-output=site\ncielo-schema=1\n";
const TEMPERATURES_DIRECTORY: &str = "temperatures";

static SITE_DIRECTORY: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/site");

/// Kind of generated artifact to publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputKind {
    Site,
    Data,
}

impl OutputKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Data => "data",
        }
    }

    const fn marker_content(self) -> &'static str {
        match self {
            Self::Site => SITE_MARKER_CONTENT,
            Self::Data => DATA_MARKER_CONTENT,
        }
    }

    fn accepts_marker(self, marker: &str) -> bool {
        marker == self.marker_content()
            || self == Self::Data && marker == LEGACY_DATA_MARKER_CONTENT
    }
}

/// Counts from a successfully published snapshot.
#[derive(Debug)]
pub(crate) struct GenerationSummary {
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

/// Fetch and transactionally publish one complete generated artifact.
pub(crate) async fn generate(
    client: &AemetClient,
    output_dir: &Path,
    output_kind: OutputKind,
) -> Result<GenerationSummary> {
    // Reject unsafe destinations before making a potentially expensive request.
    let output_dir = validate_output_directory(output_dir, output_kind)?;
    let data = client.fetch().await?;
    let snapshot = build_snapshot(data)?;
    let summary = GenerationSummary {
        municipalities: snapshot.municipalities.len(),
        temperature_files: snapshot.forecasts.len(),
    };

    // A complete sibling staging directory keeps readers away from partial data.
    let staging = write_staging_directory(&output_dir, &snapshot, output_kind)?;
    publish_staging_directory(&staging, &output_dir, output_kind)?;

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

fn write_staging_directory(
    output_dir: &Path,
    snapshot: &Snapshot,
    output_kind: OutputKind,
) -> Result<TempDir> {
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
    match output_kind {
        OutputKind::Site => {
            write_site_assets(staging.path())?;
            let data_dir = staging.path().join(DATA_DIRECTORY);
            fs::create_dir(&data_dir).context("failed to create site data directory")?;
            write_managed_marker(&data_dir, OutputKind::Data)?;
            write_data_files(&data_dir, snapshot)?;
        }
        OutputKind::Data => write_data_files(staging.path(), snapshot)?,
    }

    Ok(staging)
}

fn write_managed_marker(output_dir: &Path, output_kind: OutputKind) -> Result<()> {
    fs::write(
        output_dir.join(MANAGED_MARKER),
        output_kind.marker_content(),
    )
    .context("failed to write generated-directory marker")
}

fn write_site_assets(output_dir: &Path) -> Result<()> {
    // Keep the generated artifact independent from the source checkout.
    SITE_DIRECTORY
        .extract(output_dir)
        .context("failed to write embedded site assets")?;

    // Derive the deployable sprite from the canonical individual SVG files.
    write_icon_sprite(output_dir)
}

fn write_icon_sprite(output_dir: &Path) -> Result<()> {
    let icons_dir = output_dir.join(ICONS_DIRECTORY);
    let sprite = build_icon_sprite(&icons_dir)?;
    let sprite_path = output_dir.join(ICON_SPRITE_FILENAME);
    fs::write(&sprite_path, sprite)
        .with_context(|| format!("failed to write icon sprite {}", sprite_path.display()))
}

fn build_icon_sprite(icons_dir: &Path) -> Result<String> {
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
    let mut sprite = String::from(
        "<!-- Generated from assets/icons/*.svg; see assets/icons/LICENSE. -->\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\">\n",
    );
    for icon_path in icon_paths {
        let icon_name = icon_path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("icon filename is not valid UTF-8")?;
        let source = fs::read_to_string(&icon_path)
            .with_context(|| format!("failed to read icon {}", icon_path.display()))?;
        sprite.push_str(&build_icon_symbol(icon_name, &source)?);
    }
    sprite.push_str("</svg>\n");

    Ok(sprite)
}

fn build_icon_symbol(icon_name: &str, source: &str) -> Result<String> {
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
    let body = &source[root_open_end + 1..root_close_start];

    Ok(format!(
        "  <symbol id=\"{icon_name}\"{attributes}>{body}  </symbol>\n"
    ))
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
