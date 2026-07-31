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
    AemetClient, AemetWeatherData, DailyForecast, DailySummary, MunicipalityDailyForecast,
    MunicipalityForecast, WeatherCondition, validate_municipality_id,
};

use super::GENERATOR_IDENTITY;
use super::publisher::{OutputKind, create_staging_directory, publish_staging_directory};

const FORECAST_BUNDLE_RANGE_SIZE: u16 = 20;
const FORECASTS_DIRECTORY: &str = "forecasts";
const CATALOG_FILENAME: &str = "catalog.json";
const FORECAST_SCHEMA_VERSION: u8 = 2;

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
    summary: ForecastSummaryDocument<'a>,
    events: Vec<ForecastEventDocument<'a>>,
    hours: Vec<ForecastHourDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct ForecastSummaryDocument<'a> {
    temp_min_c: i16,
    temp_max_c: i16,

    state: Option<WeatherCondition>,
    desc: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ForecastEventDocument<'a> {
    kind: ForecastEventKind,
    time: &'a str,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum ForecastEventKind {
    #[serde(rename = "sunrise")]
    Sunrise,
    #[serde(rename = "sunset")]
    Sunset,
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
    schema_version: u8,
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
    pub(super) forecasts: Vec<MergedMunicipalityForecast>,
}

#[derive(Debug)]
pub(super) struct MergedMunicipalityForecast {
    pub(super) id: String,
    generated_at: String,
    pub(super) days: Vec<MergedForecastDay>,
}

#[derive(Debug)]
pub(super) struct MergedForecastDay {
    pub(super) summary: DailySummary,
    pub(super) hourly: Option<DailyForecast>,
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
        daily_forecasts,
    } = source_data;
    forecasts.sort_by(|left, right| left.id.cmp(&right.id));
    let mut daily_forecasts = index_daily_forecasts(daily_forecasts)?;

    let mut source_forecast_ids = HashSet::with_capacity(forecasts.len());
    let mut retained_forecast_ids = HashSet::with_capacity(forecasts.len());
    let mut municipalities = Vec::with_capacity(forecasts.len());
    let mut merged_forecasts = Vec::with_capacity(forecasts.len());
    let mut forecast_only_count = 0_usize;
    let mut missing_daily_count = 0_usize;
    let mut missing_daily_date_count = 0_usize;
    for forecast in forecasts {
        validate_municipality_id(&forecast.id)?;
        if !source_forecast_ids.insert(forecast.id.clone()) {
            bail!("duplicate forecast ID: {}", forecast.id);
        }

        let Some(daily_forecast) = daily_forecasts.remove(&forecast.id) else {
            missing_daily_count += 1;
            warn!(
                municipality_id = %forecast.id,
                municipality_name = %forecast.name,
                "excluding hourly forecast without a daily forecast"
            );
            continue;
        };
        let missing_dates = missing_daily_dates(&forecast, &daily_forecast);
        if !missing_dates.is_empty() {
            missing_daily_date_count += missing_dates.len();
            warn!(
                municipality_id = %forecast.id,
                municipality_name = %forecast.name,
                missing_dates = ?missing_dates,
                "excluding hourly forecast without matching daily summaries"
            );
            continue;
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
        retained_forecast_ids.insert(forecast.id.clone());
        merged_forecasts.push(merge_forecasts(forecast, daily_forecast)?);
    }

    let master_only_count = master_municipalities
        .keys()
        .filter(|id| !retained_forecast_ids.contains(id.as_str()))
        .count();
    let daily_only_count = daily_forecasts.len();
    if forecast_only_count > 0
        || master_only_count > 0
        || daily_only_count > 0
        || missing_daily_count > 0
        || missing_daily_date_count > 0
    {
        warn!(
            forecast_only = forecast_only_count,
            master_only = master_only_count,
            daily_only = daily_only_count,
            missing_daily = missing_daily_count,
            missing_daily_dates = missing_daily_date_count,
            "AEMET municipality products produced a reduced merged forecast set"
        );
    }
    if merged_forecasts.is_empty() {
        bail!("AEMET municipality products do not contain any mergeable forecasts");
    }

    Ok(WeatherDataSnapshot {
        municipalities,
        forecasts: merged_forecasts,
    })
}

fn index_daily_forecasts(
    daily_forecasts: Vec<MunicipalityDailyForecast>,
) -> Result<BTreeMap<String, MunicipalityDailyForecast>> {
    let mut index = BTreeMap::new();
    for forecast in daily_forecasts {
        validate_municipality_id(&forecast.id)?;
        if index.insert(forecast.id.clone(), forecast).is_some() {
            bail!("duplicate daily forecast ID");
        }
    }
    Ok(index)
}

fn missing_daily_dates(
    forecast: &MunicipalityForecast,
    daily_forecast: &MunicipalityDailyForecast,
) -> Vec<String> {
    let daily_dates = daily_forecast
        .summaries
        .iter()
        .map(|summary| summary.date.as_str())
        .collect::<HashSet<_>>();
    forecast
        .daily_forecasts
        .iter()
        .filter(|day| !daily_dates.contains(day.date.as_str()))
        .map(|day| day.date.clone())
        .collect()
}

fn merge_forecasts(
    forecast: MunicipalityForecast,
    daily_forecast: MunicipalityDailyForecast,
) -> Result<MergedMunicipalityForecast> {
    let mut hourly_by_date = forecast
        .daily_forecasts
        .into_iter()
        .map(|day| (day.date.clone(), day))
        .collect::<BTreeMap<_, _>>();
    let days = daily_forecast
        .summaries
        .into_iter()
        .map(|summary| {
            let hourly = hourly_by_date.remove(&summary.date);
            MergedForecastDay { summary, hourly }
        })
        .collect();
    if !hourly_by_date.is_empty() {
        bail!(
            "forecast {} retained hourly dates without daily summaries",
            forecast.id
        );
    }

    Ok(MergedMunicipalityForecast {
        id: forecast.id,
        generated_at: forecast.generated_at.max(daily_forecast.generated_at),
        days,
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
            .insert(&forecast.id, forecast_days(&forecast.days));
    }

    let forecast_bundle_files = bundles.len();
    for (relative_path, forecasts) in bundles {
        let document = ForecastBundleDocument {
            schema_version: FORECAST_SCHEMA_VERSION,
            forecasts,
        };
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

fn forecast_days(days: &[MergedForecastDay]) -> Vec<ForecastDayDocument<'_>> {
    days.iter()
        .map(|day| ForecastDayDocument {
            date: &day.summary.date,
            summary: ForecastSummaryDocument {
                temp_min_c: day.summary.minimum_temperature_celsius,
                temp_max_c: day.summary.maximum_temperature_celsius,
                state: day.summary.condition,
                desc: day.summary.description.as_deref(),
            },
            events: day.hourly.as_ref().map_or_else(Vec::new, |hourly| {
                vec![
                    ForecastEventDocument {
                        kind: ForecastEventKind::Sunrise,
                        time: &hourly.sunrise,
                    },
                    ForecastEventDocument {
                        kind: ForecastEventKind::Sunset,
                        time: &hourly.sunset,
                    },
                ]
            }),
            hours: day
                .hourly
                .as_ref()
                .map(|hourly| hourly.hourly_forecasts.as_slice())
                .unwrap_or_default()
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
