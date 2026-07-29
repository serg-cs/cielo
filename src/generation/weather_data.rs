use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tracing::warn;

use crate::aemet::{
    AemetClient, AemetWeatherData, DailyForecast, MunicipalityForecast, WeatherCondition,
    validate_municipality_id,
};

use super::GENERATOR_IDENTITY;
use super::publisher::{OutputKind, create_staging_directory, publish_staging_directory};

const FORECAST_BUNDLE_RANGE_SIZE: u16 = 20;
const FORECASTS_DIRECTORY: &str = "forecasts";
const CATALOG_FILENAME: &str = "catalog.json";

#[derive(Debug)]
pub(crate) struct WeatherDataGenerationSummary {
    pub(crate) municipalities: usize,
    pub(crate) forecast_bundle_files: usize,
    pub(crate) files: usize,
    pub(crate) bytes: usize,
}

#[derive(Debug, Serialize)]
struct MunicipalityCatalogDocument<'a> {
    generator: &'a str,
    updated_at: &'a str,
    provinces: Vec<ProvinceDocument<'a>>,
}

#[derive(Debug)]
pub(super) struct MunicipalityRecord {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) province: String,
    pub(super) time_zone: TimeZone,
}

#[derive(Debug, Serialize)]
struct ProvinceDocument<'a> {
    name: &'a str,
    tz: TimeZone,
    municipalities: Vec<CatalogMunicipalityDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct CatalogMunicipalityDocument<'a> {
    id: &'a str,
    name: &'a str,
}

#[derive(Debug, Serialize)]
struct ForecastDayDocument<'a> {
    date: &'a str,
    sunrise: &'a str,
    sunset: &'a str,
    hours: Vec<ForecastHourDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct ForecastHourDocument<'a> {
    hour: u8,
    temp_c: i16,
    state: WeatherCondition,
    desc: &'a str,
}

#[derive(Debug, Serialize)]
struct ForecastBundleDocument<'a> {
    forecasts: BTreeMap<&'a str, Vec<ForecastDayDocument<'a>>>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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
    let forecast_bundle_files =
        write_weather_data_files(staging.path(), &snapshot, &mut statistics)?;
    let summary = WeatherDataGenerationSummary {
        municipalities: snapshot.municipalities.len(),
        forecast_bundle_files,
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
) -> Result<usize> {
    let latest_generation_time = snapshot
        .forecasts
        .iter()
        .map(|forecast| forecast.generated_at.as_str())
        .max()
        .context("weather snapshot does not contain forecasts")?;
    let forecasts_directory = output_directory.join(FORECASTS_DIRECTORY);
    fs::create_dir(&forecasts_directory).context("failed to create forecast directory")?;

    // Publish the newest source time with the catalog for the locations footer.
    let catalog = MunicipalityCatalogDocument {
        generator: GENERATOR_IDENTITY,
        updated_at: latest_generation_time,
        provinces: group_municipalities_by_province(&snapshot.municipalities)?,
    };
    write_json(
        output_directory,
        Path::new(CATALOG_FILENAME),
        &catalog,
        statistics,
    )?;

    // Group stable numeric ID ranges before serializing each bundle once.
    let mut bundles = BTreeMap::<PathBuf, BTreeMap<&str, Vec<ForecastDayDocument>>>::new();
    for forecast in &snapshot.forecasts {
        bundles
            .entry(forecast_bundle_path(&forecast.id)?)
            .or_default()
            .insert(&forecast.id, forecast_days(&forecast.daily_forecasts));
    }

    let forecast_bundle_files = bundles.len();
    for (relative_path, forecasts) in bundles {
        let document = ForecastBundleDocument { forecasts };
        write_json(output_directory, &relative_path, &document, statistics)?;
    }
    Ok(forecast_bundle_files)
}

fn group_municipalities_by_province(
    municipalities: &[MunicipalityRecord],
) -> Result<Vec<ProvinceDocument<'_>>> {
    let mut provinces = BTreeMap::<&str, (TimeZone, Vec<CatalogMunicipalityDocument>)>::new();
    for municipality in municipalities {
        let (time_zone, records) = provinces
            .entry(&municipality.province)
            .or_insert_with(|| (municipality.time_zone, Vec::new()));
        if *time_zone != municipality.time_zone {
            bail!(
                "province {} contains municipalities in different time zones",
                municipality.province
            );
        }
        records.push(CatalogMunicipalityDocument {
            id: &municipality.id,
            name: &municipality.name,
        });
    }

    Ok(provinces
        .into_iter()
        .map(|(name, (tz, municipalities))| ProvinceDocument {
            name,
            tz,
            municipalities,
        })
        .collect())
}

fn forecast_days(daily_forecasts: &[DailyForecast]) -> Vec<ForecastDayDocument<'_>> {
    daily_forecasts
        .iter()
        .map(|daily_forecast| ForecastDayDocument {
            date: &daily_forecast.date,
            sunrise: &daily_forecast.sunrise,
            sunset: &daily_forecast.sunset,
            hours: daily_forecast
                .hourly_forecasts
                .iter()
                .map(|forecast| ForecastHourDocument {
                    hour: forecast.hour,
                    temp_c: forecast.temperature_celsius,
                    state: forecast.condition,
                    desc: &forecast.description,
                })
                .collect(),
        })
        .collect()
}

fn forecast_bundle_path(municipality_id: &str) -> Result<PathBuf> {
    validate_municipality_id(municipality_id)?;
    let province = &municipality_id[..2];
    let municipality_number = municipality_id[2..]
        .parse::<u16>()
        .with_context(|| format!("invalid municipality ID: {municipality_id}"))?;
    let range_start = municipality_number / FORECAST_BUNDLE_RANGE_SIZE * FORECAST_BUNDLE_RANGE_SIZE;

    Ok(Path::new(FORECASTS_DIRECTORY)
        .join(province)
        .join(format!("{range_start:03}.json")))
}

fn write_json(
    output_directory: &Path,
    relative_path: &Path,
    value: &impl Serialize,
    statistics: &mut WeatherDataStatistics,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .with_context(|| format!("failed to serialize {}", relative_path.display()))?;
    write_bytes(&output_directory.join(relative_path), &bytes)?;
    statistics.record(&bytes);
    Ok(())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    // Materialize nested province directories only when their bundles are present.
    let parent = path
        .parent()
        .context("generated weather-data file does not have a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create generated directory {}", parent.display()))?;
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
