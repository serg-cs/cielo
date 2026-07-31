use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{Cursor, Read},
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use tracing::warn;

use super::decoding::{decode_iso_8859_15, repair_iso_8859_15_mojibake};
use super::models::{
    DailyForecast, DailySummary, HourlyForecast, MunicipalityDailyForecast, MunicipalityForecast,
    WeatherCondition,
};

const MAX_ARCHIVE_ENTRY_SIZE: u64 = 1024 * 1024;
const MAX_DECOMPRESSED_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_FORECASTS: usize = 10_000;

#[derive(Debug, Deserialize)]
struct MunicipalityRecord {
    id: String,
    #[serde(rename = "nombre")]
    name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ForecastDocument {
    pub(super) root: ForecastRoot,
}

#[derive(Debug, Deserialize)]
pub(super) struct ForecastRoot {
    id: String,
    #[serde(rename = "elaborado")]
    generated_at: String,
    #[serde(rename = "nombre")]
    name: String,
    #[serde(rename = "provincia")]
    province: String,
    #[serde(rename = "prediccion")]
    prediction: Prediction,
}

#[derive(Debug, Deserialize)]
struct DailyForecastDocument {
    root: DailyForecastRoot,
}

#[derive(Debug, Deserialize)]
struct DailyForecastRoot {
    id: String,
    #[serde(rename = "elaborado")]
    generated_at: String,
    #[serde(rename = "prediccion")]
    prediction: DailyPrediction,
}

#[derive(Debug, Deserialize)]
struct DailyPrediction {
    #[serde(rename = "dia")]
    days: Vec<DailySourceDay>,
}

#[derive(Debug, Deserialize)]
struct DailySourceDay {
    #[serde(rename = "fecha")]
    date: String,
    #[serde(rename = "temperatura")]
    temperature: DailyTemperature,

    #[serde(
        default,
        rename = "estado_cielo",
        deserialize_with = "deserialize_one_or_many"
    )]
    sky_states: Vec<DailyForecastSkyState>,
}

#[derive(Debug, Deserialize)]
struct DailyForecastSkyState {
    #[serde(default, rename = "valor")]
    code: Option<String>,
    #[serde(default, rename = "descripcion")]
    description: Option<String>,
    #[serde(default, rename = "periodo")]
    period: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DailyTemperature {
    #[serde(rename = "minima")]
    minimum: TemperatureValue,
    #[serde(rename = "maxima")]
    maximum: TemperatureValue,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TemperatureValue {
    Integer(i64),
    Text(String),
}

#[derive(Debug, Deserialize)]
struct Prediction {
    #[serde(rename = "dia")]
    days: Vec<ForecastDay>,
}

#[derive(Debug, Deserialize)]
struct ForecastDay {
    #[serde(rename = "fecha")]
    date: String,
    #[serde(rename = "orto")]
    sunrise: String,
    #[serde(rename = "ocaso")]
    sunset: String,
    #[serde(
        default,
        rename = "estado_cielo",
        deserialize_with = "deserialize_one_or_many"
    )]
    sky_states: Vec<ForecastSkyState>,
    #[serde(
        default,
        rename = "temperatura",
        deserialize_with = "deserialize_one_or_many"
    )]
    temperatures: Vec<ForecastTemperature>,
}

#[derive(Debug, Deserialize)]
struct ForecastTemperature {
    #[serde(rename = "periodo")]
    hour: String,
    #[serde(rename = "valor")]
    celsius: String,
}

#[derive(Debug, Deserialize)]
struct ForecastSkyState {
    #[serde(rename = "periodo")]
    hour: String,
    #[serde(rename = "valor")]
    code: String,
    #[serde(rename = "descripcion")]
    description: String,
}

struct NormalizedCondition {
    condition: WeatherCondition,
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
    None(()),
}

fn deserialize_one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
        OneOrMany::None(()) => Vec::new(),
    })
}

pub(super) fn parse_municipalities(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let text = decode_iso_8859_15(bytes);
    let records: Vec<MunicipalityRecord> =
        serde_json::from_str(&text).context("invalid municipality JSON")?;
    let mut municipalities = HashMap::with_capacity(records.len());

    for record in records {
        let id = record
            .id
            .strip_prefix("id")
            .context("municipality ID does not start with 'id'")?
            .to_owned();
        validate_municipality_id(&id)?;
        let name = record.name.trim();
        if name.is_empty() {
            bail!("municipality {id} has an empty name");
        }
        if municipalities.insert(id.clone(), name.to_owned()).is_some() {
            bail!("duplicate municipality ID in master data: {id}");
        }
    }

    Ok(municipalities)
}

pub(super) fn parse_forecast_archive(bytes: &[u8]) -> Result<Vec<MunicipalityForecast>> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut forecasts = Vec::new();
    let mut ids = HashSet::new();
    let mut archive_entries = 0_usize;
    let mut total_size = 0_u64;

    for entry in archive.entries().context("invalid tar archive")? {
        if archive_entries >= MAX_FORECASTS {
            bail!("forecast archive contains too many entries");
        }
        archive_entries += 1;
        let mut entry = entry.context("invalid tar archive entry")?;
        if !entry.header().entry_type().is_file() {
            bail!("forecast archive contains a non-file entry");
        }

        let path = entry.path().context("invalid forecast archive path")?;
        let path = path
            .to_str()
            .context("forecast archive path is not valid UTF-8")?
            .to_owned();
        let filename_id = forecast_id_from_filename(&path)?;
        let size = entry
            .header()
            .size()
            .context("invalid archive entry size")?;
        if size > MAX_ARCHIVE_ENTRY_SIZE {
            bail!("forecast archive entry is too large: {path}");
        }
        total_size = total_size
            .checked_add(size)
            .context("forecast archive decompressed size overflowed")?;
        if total_size > MAX_DECOMPRESSED_ARCHIVE_SIZE {
            bail!("forecast archive is too large when decompressed");
        }

        let mut body = Vec::with_capacity(u64_to_usize(size)?);
        entry
            .read_to_end(&mut body)
            .with_context(|| format!("failed to read forecast archive entry: {path}"))?;
        let document: ForecastDocument = serde_json::from_slice(&body)
            .with_context(|| format!("invalid forecast JSON in {path}"))?;
        let forecast = normalize_forecast(document.root, &filename_id)?;

        if !ids.insert(filename_id.clone()) {
            bail!("duplicate forecast ID in archive: {filename_id}");
        }
        if let Some(forecast) = forecast {
            forecasts.push(forecast);
        }
    }

    if archive_entries == 0 {
        bail!("forecast archive is empty");
    }
    if forecasts.is_empty() {
        bail!("forecast archive does not contain any forecasts with sky conditions");
    }

    Ok(forecasts)
}

pub(super) fn parse_daily_forecast_archive(bytes: &[u8]) -> Result<Vec<MunicipalityDailyForecast>> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut forecasts = Vec::new();
    let mut ids = HashSet::new();
    let mut archive_entries = 0_usize;
    let mut total_size = 0_u64;

    for entry in archive
        .entries()
        .context("invalid daily forecast archive")?
    {
        if archive_entries >= MAX_FORECASTS {
            bail!("daily forecast archive contains too many entries");
        }
        archive_entries += 1;
        let mut entry = entry.context("invalid daily forecast archive entry")?;
        if !entry.header().entry_type().is_file() {
            bail!("daily forecast archive contains a non-file entry");
        }

        let path = entry
            .path()
            .context("invalid daily forecast archive path")?;
        let path = path
            .to_str()
            .context("daily forecast archive path is not valid UTF-8")?
            .to_owned();
        let filename_id = daily_forecast_id_from_filename(&path)?;
        let size = entry
            .header()
            .size()
            .context("invalid daily forecast archive entry size")?;
        if size > MAX_ARCHIVE_ENTRY_SIZE {
            bail!("daily forecast archive entry is too large: {path}");
        }
        total_size = total_size
            .checked_add(size)
            .context("daily forecast archive decompressed size overflowed")?;
        if total_size > MAX_DECOMPRESSED_ARCHIVE_SIZE {
            bail!("daily forecast archive is too large when decompressed");
        }

        let mut body = Vec::with_capacity(u64_to_usize(size)?);
        entry
            .read_to_end(&mut body)
            .with_context(|| format!("failed to read daily forecast archive entry: {path}"))?;
        let document: DailyForecastDocument = serde_json::from_slice(&body)
            .with_context(|| format!("invalid daily forecast JSON in {path}"))?;
        let forecast = normalize_daily_forecast(document.root, &filename_id)?;

        if !ids.insert(filename_id.clone()) {
            bail!("duplicate daily forecast ID in archive: {filename_id}");
        }
        forecasts.push(forecast);
    }

    if archive_entries == 0 {
        bail!("daily forecast archive is empty");
    }
    if forecasts.is_empty() {
        bail!("daily forecast archive does not contain any forecasts");
    }

    Ok(forecasts)
}

fn normalize_daily_forecast(
    root: DailyForecastRoot,
    filename_id: &str,
) -> Result<MunicipalityDailyForecast> {
    validate_municipality_id(&root.id)?;
    if root.id != filename_id {
        bail!(
            "daily forecast filename ID {filename_id} does not match document ID {}",
            root.id
        );
    }
    if root.generated_at.trim().is_empty() {
        bail!("daily forecast {} has an empty generation time", root.id);
    }

    // Normalize daily values into a stable date index before publishing them.
    let mut summaries = BTreeMap::new();
    for day in root.prediction.days {
        validate_date(&day.date)?;
        let condition = select_daily_condition(
            day.sky_states,
            root.generated_at.starts_with(&day.date),
            &root.id,
        )?;
        let minimum_temperature_celsius = day
            .temperature
            .minimum
            .parse_i16()
            .with_context(|| format!("invalid minimum temperature in forecast {}", root.id))?;
        let maximum_temperature_celsius = day
            .temperature
            .maximum
            .parse_i16()
            .with_context(|| format!("invalid maximum temperature in forecast {}", root.id))?;
        if minimum_temperature_celsius > maximum_temperature_celsius {
            bail!(
                "daily forecast {} has a minimum temperature above its maximum on {}",
                root.id,
                day.date
            );
        }
        let summary = DailySummary {
            date: day.date.clone(),
            minimum_temperature_celsius,
            maximum_temperature_celsius,
            condition: condition.as_ref().map(|value| value.condition),
            description: condition.map(|value| value.description),
        };
        if summaries.insert(day.date, summary).is_some() {
            bail!("daily forecast {} contains duplicate days", root.id);
        }
    }
    if summaries.is_empty() {
        bail!("daily forecast {} does not contain any days", root.id);
    }

    Ok(MunicipalityDailyForecast {
        id: root.id,
        generated_at: root.generated_at,
        summaries: summaries.into_values().collect(),
    })
}

fn select_daily_condition(
    sky_states: Vec<DailyForecastSkyState>,
    allow_partial_fallback: bool,
    municipality_id: &str,
) -> Result<Option<NormalizedCondition>> {
    let mut whole_day = None;
    let mut unperioded = None;
    let mut partial_periods = HashSet::new();
    let mut partial = Vec::new();

    // Index the semantic whole-day forms before considering horizon-specific periods.
    for sky_state in sky_states {
        let period = sky_state
            .period
            .as_deref()
            .map(str::trim)
            .map(str::to_owned);
        match period.as_deref() {
            Some("00-24") => {
                if whole_day.replace(sky_state).is_some() {
                    bail!("daily forecast {municipality_id} contains duplicate 00-24 conditions");
                }
            }
            None => {
                if unperioded.replace(sky_state).is_some() {
                    bail!(
                        "daily forecast {municipality_id} contains duplicate unperioded conditions"
                    );
                }
            }
            Some(period) => {
                let Some((duration, end_hour)) = daily_partial_period_rank(period) else {
                    continue;
                };
                if daily_sky_state_is_empty(&sky_state) {
                    continue;
                }
                if !partial_periods.insert(period.to_owned()) {
                    bail!(
                        "daily forecast {municipality_id} contains duplicate {period} conditions"
                    );
                }
                partial.push((duration, end_hour, sky_state));
            }
        }
    }

    if let Some(condition) = normalize_daily_condition(whole_day, municipality_id)? {
        return Ok(Some(condition));
    }
    if let Some(condition) = normalize_daily_condition(unperioded, municipality_id)? {
        return Ok(Some(condition));
    }
    if !allow_partial_fallback {
        return Ok(None);
    }

    // Prefer the widest remaining period, breaking equal spans toward the end of the day.
    partial.sort_by_key(|(duration, end_hour, _)| (*duration, *end_hour));
    let selected = partial.pop().map(|(_, _, sky_state)| sky_state);
    normalize_daily_condition(selected, municipality_id)
}

fn normalize_daily_condition(
    sky_state: Option<DailyForecastSkyState>,
    municipality_id: &str,
) -> Result<Option<NormalizedCondition>> {
    let Some(sky_state) = sky_state else {
        return Ok(None);
    };
    let code = sky_state.code.as_deref().unwrap_or_default().trim();
    let description =
        repair_iso_8859_15_mojibake(sky_state.description.as_deref().unwrap_or_default().trim());
    if code.is_empty() && description.is_empty() {
        return Ok(None);
    }
    if code.is_empty() {
        bail!("empty daily condition code in forecast {municipality_id}");
    }
    let condition = WeatherCondition::from_aemet_code(code).with_context(|| {
        format!("unknown daily condition code in forecast {municipality_id}: {code}")
    })?;
    if description.is_empty() {
        bail!("empty daily condition description in forecast {municipality_id}");
    }

    Ok(Some(NormalizedCondition {
        condition,
        description,
    }))
}

fn daily_sky_state_is_empty(sky_state: &DailyForecastSkyState) -> bool {
    sky_state
        .code
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && sky_state
            .description
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
}

fn daily_partial_period_rank(period: &str) -> Option<(u8, u8)> {
    match period {
        "00-06" => Some((6, 6)),
        "06-12" => Some((6, 12)),
        "12-18" => Some((6, 18)),
        "18-24" => Some((6, 24)),
        "00-12" => Some((12, 12)),
        "12-24" => Some((12, 24)),
        _ => None,
    }
}

impl TemperatureValue {
    fn parse_i16(self) -> Result<i16> {
        match self {
            Self::Integer(value) => {
                i16::try_from(value).context("temperature is outside the supported range")
            }
            Self::Text(value) => value
                .parse::<i16>()
                .context("temperature is not an integer"),
        }
    }
}

pub(super) fn normalize_forecast(
    root: ForecastRoot,
    filename_id: &str,
) -> Result<Option<MunicipalityForecast>> {
    validate_municipality_id(&root.id)?;
    if root.id != filename_id {
        bail!(
            "forecast filename ID {filename_id} does not match document ID {}",
            root.id
        );
    }
    if root.generated_at.trim().is_empty() {
        bail!("forecast {} has an empty generation time", root.id);
    }

    // Normalize conditions independently so source array order cannot affect joins.
    let mut conditions = BTreeMap::new();
    let mut daily_forecasts = BTreeMap::new();
    let mut temperature_values = Vec::new();
    for day in root.prediction.days {
        let ForecastDay {
            date,
            sunrise,
            sunset,
            sky_states,
            temperatures,
        } = day;
        insert_daily_forecast(&mut daily_forecasts, &root.id, &date, sunrise, sunset)?;

        for value in sky_states {
            let hour = value
                .hour
                .parse::<u8>()
                .with_context(|| format!("invalid condition hour in forecast {}", root.id))?;
            if hour > 23 {
                bail!("invalid condition hour in forecast {}: {hour}", root.id);
            }

            let code = value.code.trim();
            if code.is_empty() {
                bail!("empty condition code in forecast {}", root.id);
            }
            let condition = WeatherCondition::from_aemet_code(code).with_context(|| {
                format!("unknown condition code in forecast {}: {code}", root.id)
            })?;
            let description = repair_iso_8859_15_mojibake(value.description.trim());
            if description.is_empty() {
                bail!("empty condition description in forecast {}", root.id);
            }

            let key = (date.clone(), hour);
            if conditions
                .insert(
                    key,
                    NormalizedCondition {
                        condition,
                        description,
                    },
                )
                .is_some()
            {
                bail!("forecast {} contains duplicate condition hours", root.id);
            }
        }

        for value in temperatures {
            let hour = value
                .hour
                .parse::<u8>()
                .with_context(|| format!("invalid hour in forecast {}", root.id))?;
            if hour > 23 {
                bail!("invalid hour in forecast {}: {hour}", root.id);
            }
            let celsius = value
                .celsius
                .parse::<i16>()
                .with_context(|| format!("invalid temperature in forecast {}", root.id))?;
            temperature_values.push((date.clone(), hour, celsius));
        }
    }
    if conditions.is_empty() {
        warn!(
            municipality_id = %root.id,
            municipality_name = %root.name,
            "excluding AEMET forecast without sky conditions"
        );
        return Ok(None);
    }
    if temperature_values.is_empty() {
        bail!("forecast {} does not contain temperatures", root.id);
    }

    let daily_forecasts =
        join_hourly_forecasts(daily_forecasts, &conditions, temperature_values, &root.id)?;

    Ok(Some(MunicipalityForecast {
        id: root.id,
        name: repair_iso_8859_15_mojibake(&root.name),
        province: repair_iso_8859_15_mojibake(&root.province),
        generated_at: root.generated_at,
        daily_forecasts,
    }))
}

fn join_hourly_forecasts(
    mut daily_forecasts: BTreeMap<String, DailyForecast>,
    conditions: &BTreeMap<(String, u8), NormalizedCondition>,
    mut temperature_values: Vec<(String, u8, i16)>,
    municipality_id: &str,
) -> Result<Vec<DailyForecast>> {
    temperature_values.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    if temperature_values
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1)
    {
        bail!("forecast {municipality_id} contains duplicate temperature hours");
    }

    // Prefer exact or earlier conditions, falling forward only before the first state.
    for (date, hour, temperature_celsius) in temperature_values {
        let key = (date.clone(), hour);
        let condition = condition_at_or_near(conditions, &key)
            .context("forecast condition index unexpectedly became empty")?;
        let daily_forecast = daily_forecasts
            .get_mut(&date)
            .context("forecast day index unexpectedly became incomplete")?;
        daily_forecast.hourly_forecasts.push(HourlyForecast {
            hour,
            temperature_celsius,
            condition: condition.condition,
            description: condition.description.clone(),
        });
    }

    Ok(daily_forecasts
        .into_values()
        .filter(|forecast| !forecast.hourly_forecasts.is_empty())
        .collect())
}

fn insert_daily_forecast(
    daily_forecasts: &mut BTreeMap<String, DailyForecast>,
    municipality_id: &str,
    date: &str,
    sunrise: String,
    sunset: String,
) -> Result<()> {
    validate_date(date)?;
    validate_solar_time(&sunrise)
        .with_context(|| format!("invalid sunrise time in forecast {municipality_id}"))?;
    validate_solar_time(&sunset)
        .with_context(|| format!("invalid sunset time in forecast {municipality_id}"))?;
    if sunrise >= sunset {
        bail!("sunrise is not before sunset in forecast {municipality_id}");
    }

    let daily_forecast = DailyForecast {
        date: date.to_owned(),
        sunrise,
        sunset,
        hourly_forecasts: Vec::new(),
    };
    if daily_forecasts
        .insert(date.to_owned(), daily_forecast)
        .is_some()
    {
        bail!("forecast {municipality_id} contains duplicate days");
    }
    Ok(())
}

fn condition_at_or_near<'a>(
    conditions: &'a BTreeMap<(String, u8), NormalizedCondition>,
    key: &(String, u8),
) -> Option<&'a NormalizedCondition> {
    conditions
        .get(key)
        .or_else(|| {
            conditions
                .range(..key.clone())
                .next_back()
                .map(|(_, condition)| condition)
        })
        .or_else(|| {
            conditions
                .range(key.clone()..)
                .next()
                .map(|(_, condition)| condition)
        })
}

fn forecast_id_from_filename(path: &str) -> Result<String> {
    let id = path
        .strip_prefix("localidad_h_")
        .and_then(|value| value.strip_suffix(".json"))
        .with_context(|| format!("unexpected forecast archive entry: {path}"))?;
    validate_municipality_id(id)?;
    Ok(id.to_owned())
}

fn daily_forecast_id_from_filename(path: &str) -> Result<String> {
    let id = path
        .strip_prefix("localidad_")
        .and_then(|value| value.strip_suffix(".json"))
        .with_context(|| format!("unexpected daily forecast archive entry: {path}"))?;
    validate_municipality_id(id)?;
    Ok(id.to_owned())
}

pub(crate) fn validate_municipality_id(id: &str) -> Result<()> {
    if id.len() != 5 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid municipality ID: {id}");
    }
    Ok(())
}

fn validate_date(date: &str) -> Result<()> {
    let valid_shape = date.len() == 10
        && date.is_ascii()
        && date.as_bytes().get(4) == Some(&b'-')
        && date.as_bytes().get(7) == Some(&b'-');
    if !valid_shape {
        bail!("invalid forecast date: {date}");
    }

    let year = date[0..4]
        .parse::<u16>()
        .with_context(|| format!("invalid forecast date: {date}"))?;
    let month = date[5..7]
        .parse::<u8>()
        .with_context(|| format!("invalid forecast date: {date}"))?;
    let day = date[8..10]
        .parse::<u8>()
        .with_context(|| format!("invalid forecast date: {date}"))?;
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > maximum_day {
        bail!("invalid forecast date: {date}");
    }

    Ok(())
}

fn validate_solar_time(time: &str) -> Result<()> {
    let valid_shape = time.len() == 5 && time.is_ascii() && time.as_bytes().get(2) == Some(&b':');
    if !valid_shape {
        bail!("invalid solar time: {time}");
    }

    let hour = time[0..2]
        .parse::<u8>()
        .with_context(|| format!("invalid solar time: {time}"))?;
    let minute = time[3..5]
        .parse::<u8>()
        .with_context(|| format!("invalid solar time: {time}"))?;
    if hour > 23 || minute > 59 {
        bail!("invalid solar time: {time}");
    }
    Ok(())
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn u64_to_usize(value: u64) -> Result<usize> {
    usize::try_from(value).context("archive entry does not fit in memory")
}
