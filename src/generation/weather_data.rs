use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tracing::warn;

use crate::aemet::{
    AemetClient, AemetWeatherData, HourlyForecast, MunicipalityForecast, validate_municipality_id,
};

use super::GENERATOR_IDENTITY;
use super::publisher::{OutputKind, create_staging_directory, publish_staging_directory};

const AEMET_SOURCE_NAME: &str = "AEMET";
const AEMET_SOURCE_URL: &str = "https://opendata.aemet.es/";
const HOURLY_FORECASTS_DIRECTORY: &str = "hourly_forecasts";
const MUNICIPALITIES_FILENAME: &str = "municipalities.json";

#[derive(Debug)]
pub(crate) struct WeatherDataGenerationSummary {
    pub(crate) municipalities: usize,
    pub(crate) forecast_files: usize,
    pub(crate) files: usize,
    pub(crate) bytes: usize,
}

#[derive(Debug, Serialize)]
struct Source<'a> {
    name: &'a str,
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    generated_at: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct MunicipalityCatalogDocument<'a> {
    generator: &'a str,
    source: Source<'a>,
    municipalities: &'a [MunicipalityRecord],
}

#[derive(Debug, Serialize)]
pub(super) struct MunicipalityRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) province: String,
    pub(super) time_zone: TimeZone,
}

#[derive(Debug, Serialize)]
struct MunicipalityForecastDocument<'a> {
    generator: &'a str,
    source: Source<'a>,
    municipality_id: &'a str,
    time_zone: TimeZone,
    hourly_forecasts: &'a [HourlyForecast],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(super) enum TimeZone {
    #[serde(rename = "Africa/Ceuta")]
    AfricaCeuta,
    #[serde(rename = "Atlantic/Canary")]
    AtlanticCanary,
    #[serde(rename = "Europe/Madrid")]
    EuropeMadrid,
}

#[derive(Debug)]
pub(super) struct WeatherDataSnapshot {
    pub(super) municipalities: Vec<MunicipalityRecord>,
    pub(super) forecasts: Vec<MunicipalityForecast>,
}

pub(crate) async fn generate_weather_data(
    client: &AemetClient,
    output_directory: &Path,
) -> Result<WeatherDataGenerationSummary> {
    let (output_directory, staging) = create_staging_directory(output_directory, OutputKind::Data)?;
    let source_data = client.fetch().await?;
    let snapshot = build_snapshot(source_data)?;

    // Write a complete sibling output before replacing the published directory.
    let mut statistics = WeatherDataStatistics::default();
    write_weather_data_files(staging.path(), &snapshot, &mut statistics)?;
    let summary = WeatherDataGenerationSummary {
        municipalities: snapshot.municipalities.len(),
        forecast_files: snapshot.forecasts.len(),
        files: statistics.file_count,
        bytes: statistics.total_bytes,
    };

    publish_staging_directory(&staging, &output_directory, OutputKind::Data)?;
    Ok(summary)
}

pub(super) fn build_snapshot(source_data: AemetWeatherData) -> Result<WeatherDataSnapshot> {
    let AemetWeatherData {
        municipalities: master_municipalities,
        mut forecasts,
    } = source_data;
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

        municipalities.push(MunicipalityRecord {
            id: forecast.id.clone(),
            name,
            province: province.to_owned(),
            time_zone: time_zone_for(&forecast.id),
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
    Ok(WeatherDataSnapshot {
        municipalities,
        forecasts,
    })
}

pub(super) fn write_weather_data_files(
    output_directory: &Path,
    snapshot: &WeatherDataSnapshot,
    statistics: &mut WeatherDataStatistics,
) -> Result<()> {
    let latest_generation_time = snapshot
        .forecasts
        .iter()
        .map(|forecast| forecast.generated_at.as_str())
        .max()
        .context("weather snapshot does not contain forecasts")?;
    let hourly_forecasts_directory = output_directory.join(HOURLY_FORECASTS_DIRECTORY);
    fs::create_dir(&hourly_forecasts_directory)
        .context("failed to create hourly forecast directory")?;

    // Publish the newest source time with the catalog for the locations footer.
    let catalog = MunicipalityCatalogDocument {
        generator: GENERATOR_IDENTITY,
        source: Source {
            name: AEMET_SOURCE_NAME,
            url: AEMET_SOURCE_URL,
            generated_at: Some(latest_generation_time),
        },
        municipalities: &snapshot.municipalities,
    };
    write_json(
        output_directory,
        Path::new(MUNICIPALITIES_FILENAME),
        &catalog,
        statistics,
    )?;

    for forecast in &snapshot.forecasts {
        let document = MunicipalityForecastDocument {
            generator: GENERATOR_IDENTITY,
            source: Source {
                name: AEMET_SOURCE_NAME,
                url: AEMET_SOURCE_URL,
                generated_at: Some(&forecast.generated_at),
            },
            municipality_id: &forecast.id,
            time_zone: time_zone_for(&forecast.id),
            hourly_forecasts: &forecast.hourly_forecasts,
        };
        write_json(
            output_directory,
            &Path::new(HOURLY_FORECASTS_DIRECTORY).join(format!("{}.json", forecast.id)),
            &document,
            statistics,
        )?;
    }
    Ok(())
}

fn write_json(
    output_directory: &Path,
    relative_path: &Path,
    value: &impl Serialize,
    statistics: &mut WeatherDataStatistics,
) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)
        .with_context(|| format!("failed to serialize {}", relative_path.display()))?;
    bytes.push(b'\n');
    write_bytes(&output_directory.join(relative_path), &bytes)?;
    statistics.record(&bytes);
    Ok(())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create generated file {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(bytes)
        .with_context(|| format!("failed to write generated file {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("failed to flush generated file {}", path.display()))
}

#[derive(Default)]
pub(super) struct WeatherDataStatistics {
    file_count: usize,
    total_bytes: usize,
}

impl WeatherDataStatistics {
    fn record(&mut self, bytes: &[u8]) {
        self.file_count += 1;
        self.total_bytes += bytes.len();
    }
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

fn time_zone_for(municipality_id: &str) -> TimeZone {
    match municipality_id.get(..2) {
        Some("35" | "38") => TimeZone::AtlanticCanary,
        Some("51" | "52") => TimeZone::AfricaCeuta,
        _ => TimeZone::EuropeMadrid,
    }
}
