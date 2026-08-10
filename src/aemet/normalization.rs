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
    PrecipitationAmount, PrecipitationProbability, WeatherCondition,
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
    #[serde(
        default,
        rename = "prob_precipitacion",
        deserialize_with = "deserialize_one_or_many"
    )]
    precipitation_probabilities: Vec<ForecastPeriodValue>,
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
    #[serde(
        default,
        rename = "precipitacion",
        deserialize_with = "deserialize_one_or_many"
    )]
    precipitation_amounts: Vec<ForecastPeriodValue>,
    #[serde(
        default,
        rename = "prob_precipitacion",
        deserialize_with = "deserialize_one_or_many"
    )]
    precipitation_probabilities: Vec<ForecastPeriodValue>,
    #[serde(
        default,
        rename = "viento",
        deserialize_with = "deserialize_one_or_many"
    )]
    winds: Vec<ForecastWind>,
    #[serde(
        default,
        rename = "racha_max",
        deserialize_with = "deserialize_one_or_many"
    )]
    maximum_gusts: Vec<ForecastPeriodValue>,
    #[serde(
        default,
        rename = "humedad_relativa",
        deserialize_with = "deserialize_one_or_many"
    )]
    relative_humidity: Vec<ForecastPeriodValue>,
    #[serde(
        default,
        rename = "sens_termica",
        deserialize_with = "deserialize_one_or_many"
    )]
    apparent_temperatures: Vec<ForecastPeriodValue>,
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

#[derive(Debug, Deserialize)]
#[serde(from = "ForecastPeriodValueSource")]
struct ForecastPeriodValue {
    period: Option<String>,
    value: Option<SourceValue>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ForecastPeriodValueSource {
    Detailed {
        #[serde(default, rename = "periodo")]
        period: Option<String>,
        #[serde(default, rename = "valor", alias = "value")]
        value: Option<SourceValue>,
    },
    Unperioded(SourceValue),
}

#[derive(Debug, Deserialize)]
struct ForecastWind {
    #[serde(rename = "periodo")]
    period: String,
    #[serde(rename = "direccion", deserialize_with = "deserialize_one_or_many")]
    directions: Vec<SourceValue>,
    #[serde(rename = "velocidad", deserialize_with = "deserialize_one_or_many")]
    speeds: Vec<SourceValue>,
}

impl From<ForecastPeriodValueSource> for ForecastPeriodValue {
    fn from(source: ForecastPeriodValueSource) -> Self {
        match source {
            ForecastPeriodValueSource::Detailed { period, value } => Self { period, value },
            ForecastPeriodValueSource::Unperioded(value) => Self {
                period: None,
                value: Some(value),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SourceValue {
    Integer(i64),
    Decimal(f64),
    Text(String),
}

struct NormalizedCondition {
    condition: WeatherCondition,
    description: String,
}

#[derive(Debug)]
struct NormalizedWind {
    direction: Option<String>,
    speed_kilometres_per_hour: Option<u16>,
}

#[derive(Default)]
struct HourlyMeasurementIndex {
    precipitation_amounts: BTreeMap<(String, u8), PrecipitationAmount>,
    precipitation_amount_keys: HashSet<(String, u8)>,
    winds: BTreeMap<(String, u8), NormalizedWind>,
    maximum_gusts: BTreeMap<(String, u8), u16>,
    maximum_gust_keys: HashSet<(String, u8)>,
    relative_humidity: BTreeMap<(String, u8), u8>,
    relative_humidity_keys: HashSet<(String, u8)>,
    apparent_temperatures: BTreeMap<(String, u8), i16>,
}

struct HourlyMeasurementValues {
    precipitation_amounts: Vec<ForecastPeriodValue>,
    winds: Vec<ForecastWind>,
    maximum_gusts: Vec<ForecastPeriodValue>,
    relative_humidity: Vec<ForecastPeriodValue>,
    apparent_temperatures: Vec<ForecastPeriodValue>,
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
        let DailySourceDay {
            date,
            temperature,
            sky_states,
            precipitation_probabilities,
        } = day;
        validate_date(&date)?;
        let is_current_day = root.generated_at.starts_with(&date);
        let condition = select_daily_condition(sky_states, is_current_day, &root.id)?;
        let precipitation_probability = select_daily_precipitation_probability(
            precipitation_probabilities,
            is_current_day,
            &root.id,
        )?;
        let minimum_temperature_celsius = temperature
            .minimum
            .parse_i16()
            .with_context(|| format!("invalid minimum temperature in forecast {}", root.id))?;
        let maximum_temperature_celsius = temperature
            .maximum
            .parse_i16()
            .with_context(|| format!("invalid maximum temperature in forecast {}", root.id))?;
        if minimum_temperature_celsius > maximum_temperature_celsius {
            bail!(
                "daily forecast {} has a minimum temperature above its maximum on {}",
                root.id,
                date
            );
        }
        let summary = DailySummary {
            date: date.clone(),
            minimum_temperature_celsius,
            maximum_temperature_celsius,
            condition: condition.as_ref().map(|value| value.condition),
            description: condition.map(|value| value.description),
            precipitation_probability,
        };
        if summaries.insert(date, summary).is_some() {
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

fn select_daily_precipitation_probability(
    values: Vec<ForecastPeriodValue>,
    allow_partial_fallback: bool,
    municipality_id: &str,
) -> Result<Option<PrecipitationProbability>> {
    let mut whole_day = None;
    let mut unperioded = None;
    let mut seen_periods = HashSet::new();
    let mut saw_unperioded = false;
    let mut partial = Vec::new();

    // Prefer a real whole-day value and retain a current-day partial only as fallback.
    for value in values {
        let period = value
            .period
            .as_deref()
            .map(str::trim)
            .filter(|period| !period.is_empty());
        if let Some(period) = period {
            if !seen_periods.insert(period.to_owned()) {
                bail!(
                    "daily forecast {municipality_id} contains duplicate {period} precipitation probabilities"
                );
            }
        } else if saw_unperioded {
            bail!(
                "daily forecast {municipality_id} contains duplicate unperioded precipitation probabilities"
            );
        } else {
            saw_unperioded = true;
        }

        let percent = normalize_probability_percent(value.value, municipality_id)?;
        match period {
            Some("00-24") => {
                whole_day = percent.map(|percent| PrecipitationProbability {
                    period: "00-24".to_owned(),
                    percent,
                });
            }
            None => {
                unperioded = percent.map(|percent| PrecipitationProbability {
                    period: "00-24".to_owned(),
                    percent,
                });
            }
            Some(period) => {
                let Some((duration, end_hour)) = daily_partial_period_rank(period) else {
                    if percent.is_some() {
                        bail!(
                            "daily forecast {municipality_id} contains an invalid precipitation probability period: {period}"
                        );
                    }
                    continue;
                };
                if let Some(percent) = percent {
                    partial.push((
                        duration,
                        end_hour,
                        PrecipitationProbability {
                            period: period.to_owned(),
                            percent,
                        },
                    ));
                }
            }
        }
    }

    if whole_day.is_some() {
        return Ok(whole_day);
    }
    if unperioded.is_some() {
        return Ok(unperioded);
    }
    if !allow_partial_fallback {
        return Ok(None);
    }

    partial.sort_by_key(|(duration, end_hour, _)| (*duration, *end_hour));
    Ok(partial.pop().map(|(_, _, probability)| probability))
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

fn normalize_precipitation_amount(
    value: Option<SourceValue>,
    municipality_id: &str,
) -> Result<Option<PrecipitationAmount>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if let SourceValue::Text(text) = &value {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        if text.eq_ignore_ascii_case("Ip") {
            return Ok(Some(PrecipitationAmount::Trace));
        }
    }

    let tenths = value
        .precipitation_tenths()
        .with_context(|| format!("invalid precipitation amount in forecast {municipality_id}"))?;
    Ok(Some(PrecipitationAmount::MeasuredTenthsOfMillimetre(
        tenths,
    )))
}

fn normalize_probability_percent(
    value: Option<SourceValue>,
    municipality_id: &str,
) -> Result<Option<u8>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty_text() {
        return Ok(None);
    }

    let percent = value.probability_percent().with_context(|| {
        format!("invalid precipitation probability in forecast {municipality_id}")
    })?;
    if percent > 100 {
        bail!("invalid precipitation probability in forecast {municipality_id}");
    }
    Ok(Some(percent))
}

fn normalize_hourly_precipitation_probabilities(
    values: Vec<ForecastPeriodValue>,
    municipality_id: &str,
) -> Result<Vec<PrecipitationProbability>> {
    let mut seen_periods = HashSet::new();
    let mut probabilities = Vec::new();

    for value in values {
        let source_period = value
            .period
            .as_deref()
            .map(str::trim)
            .context("hourly precipitation probability does not contain a period")?;
        let period = normalize_hourly_probability_period(source_period, municipality_id)?;
        if !seen_periods.insert(period.clone()) {
            bail!(
                "forecast {municipality_id} contains duplicate precipitation probability periods"
            );
        }
        if let Some(percent) = normalize_probability_percent(value.value, municipality_id)? {
            probabilities.push(PrecipitationProbability { period, percent });
        }
    }

    probabilities.sort_by(|left, right| left.period.cmp(&right.period));
    Ok(probabilities)
}

fn normalize_hourly_probability_period(period: &str, municipality_id: &str) -> Result<String> {
    let valid_shape = period.len() == 4 && period.bytes().all(|byte| byte.is_ascii_digit());
    if !valid_shape {
        bail!(
            "forecast {municipality_id} contains an invalid precipitation probability period: {period}"
        );
    }

    let start = period[..2].parse::<u8>().with_context(|| {
        format!("invalid precipitation probability period in forecast {municipality_id}")
    })?;
    let end = period[2..].parse::<u8>().with_context(|| {
        format!("invalid precipitation probability period in forecast {municipality_id}")
    })?;
    let duration = (end + 24 - start) % 24;
    if start > 23 || end > 23 || duration != 6 {
        bail!(
            "forecast {municipality_id} contains an invalid precipitation probability period: {period}"
        );
    }

    Ok(format!("{start:02}-{end:02}"))
}

impl SourceValue {
    fn precipitation_tenths(&self) -> Option<u16> {
        match self {
            Self::Integer(value) => u16::try_from(*value).ok()?.checked_mul(10),
            Self::Decimal(value) if value.is_finite() => {
                parse_precipitation_tenths(&value.to_string())
            }
            Self::Decimal(_) => None,
            Self::Text(value) => parse_precipitation_tenths(value),
        }
    }

    fn probability_percent(&self) -> Option<u8> {
        match self {
            Self::Integer(value) => u8::try_from(*value).ok(),
            Self::Decimal(value) if value.is_finite() && value.fract() == 0.0 => {
                value.to_string().parse().ok()
            }
            Self::Decimal(_) => None,
            Self::Text(value) => value.trim().parse().ok(),
        }
    }

    fn non_negative_integer(&self) -> Option<u16> {
        match self {
            Self::Integer(value) => u16::try_from(*value).ok(),
            Self::Decimal(value) if value.is_finite() && value.fract() == 0.0 => {
                value.to_string().parse().ok()
            }
            Self::Decimal(_) => None,
            Self::Text(value) => value.trim().parse().ok(),
        }
    }

    fn signed_integer(&self) -> Option<i16> {
        match self {
            Self::Integer(value) => i16::try_from(*value).ok(),
            Self::Decimal(value) if value.is_finite() && value.fract() == 0.0 => {
                value.to_string().parse().ok()
            }
            Self::Decimal(_) => None,
            Self::Text(value) => value.trim().parse().ok(),
        }
    }

    fn is_empty_text(&self) -> bool {
        matches!(self, Self::Text(value) if value.trim().is_empty())
    }
}

fn parse_precipitation_tenths(value: &str) -> Option<u16> {
    let value = value.trim();
    let (whole, fractional) = value.split_once('.').map_or((value, ""), |parts| parts);
    let whole = whole.parse::<u16>().ok()?;
    let fractional_tenth = match fractional.as_bytes() {
        [] => 0,
        [tenth, rest @ ..] if tenth.is_ascii_digit() && rest.iter().all(|digit| *digit == b'0') => {
            u16::from(tenth - b'0')
        }
        _ => return None,
    };

    whole.checked_mul(10)?.checked_add(fractional_tenth)
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
    let mut measurements = HourlyMeasurementIndex::default();
    let mut temperature_values = Vec::new();
    for day in root.prediction.days {
        let ForecastDay {
            date,
            sunrise,
            sunset,
            sky_states,
            temperatures,
            precipitation_amounts: source_precipitation_amounts,
            precipitation_probabilities,
            winds,
            maximum_gusts,
            relative_humidity: source_relative_humidity,
            apparent_temperatures,
        } = day;
        let precipitation_probabilities =
            normalize_hourly_precipitation_probabilities(precipitation_probabilities, &root.id)?;
        insert_daily_forecast(
            &mut daily_forecasts,
            &root.id,
            &date,
            sunrise,
            sunset,
            precipitation_probabilities,
        )?;

        insert_hourly_conditions(&mut conditions, sky_states, &date, &root.id)?;

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

        insert_hourly_measurements(
            &mut measurements,
            HourlyMeasurementValues {
                precipitation_amounts: source_precipitation_amounts,
                winds,
                maximum_gusts,
                relative_humidity: source_relative_humidity,
                apparent_temperatures,
            },
            &date,
            &root.id,
        )?;
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

    let daily_forecasts = join_hourly_forecasts(
        daily_forecasts,
        &conditions,
        &measurements,
        temperature_values,
        &root.id,
    )?;

    Ok(Some(MunicipalityForecast {
        id: root.id,
        name: repair_iso_8859_15_mojibake(&root.name),
        province: repair_iso_8859_15_mojibake(&root.province),
        generated_at: root.generated_at,
        daily_forecasts,
    }))
}

fn insert_hourly_conditions(
    conditions: &mut BTreeMap<(String, u8), NormalizedCondition>,
    values: Vec<ForecastSkyState>,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    for value in values {
        let hour = value
            .hour
            .parse::<u8>()
            .with_context(|| format!("invalid condition hour in forecast {municipality_id}"))?;
        if hour > 23 {
            bail!("invalid condition hour in forecast {municipality_id}: {hour}");
        }

        let code = value.code.trim();
        if code.is_empty() {
            bail!("empty condition code in forecast {municipality_id}");
        }
        let condition = WeatherCondition::from_aemet_code(code).with_context(|| {
            format!("unknown condition code in forecast {municipality_id}: {code}")
        })?;
        let description = repair_iso_8859_15_mojibake(value.description.trim());
        if description.is_empty() {
            bail!("empty condition description in forecast {municipality_id}");
        }

        let key = (date.to_owned(), hour);
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
            bail!("forecast {municipality_id} contains duplicate condition hours");
        }
    }
    Ok(())
}

fn insert_hourly_precipitation_amounts(
    amounts: &mut BTreeMap<(String, u8), PrecipitationAmount>,
    amount_keys: &mut HashSet<(String, u8)>,
    values: Vec<ForecastPeriodValue>,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    for value in values {
        let period = value
            .period
            .as_deref()
            .map(str::trim)
            .context("hourly precipitation amount does not contain a period")?;
        let hour = period
            .parse::<u8>()
            .with_context(|| format!("invalid precipitation hour in forecast {municipality_id}"))?;
        if hour > 23 {
            bail!("invalid precipitation hour in forecast {municipality_id}: {hour}");
        }

        let key = (date.to_owned(), hour);
        if !amount_keys.insert(key.clone()) {
            bail!("forecast {municipality_id} contains duplicate precipitation amount hours");
        }
        if let Some(amount) = normalize_precipitation_amount(value.value, municipality_id)? {
            amounts.insert(key, amount);
        }
    }
    Ok(())
}

fn insert_hourly_measurements(
    measurements: &mut HourlyMeasurementIndex,
    values: HourlyMeasurementValues,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    let HourlyMeasurementValues {
        precipitation_amounts,
        winds,
        maximum_gusts,
        relative_humidity,
        apparent_temperatures,
    } = values;
    insert_hourly_precipitation_amounts(
        &mut measurements.precipitation_amounts,
        &mut measurements.precipitation_amount_keys,
        precipitation_amounts,
        date,
        municipality_id,
    )?;
    insert_hourly_wind_and_maximum_gusts(
        &mut measurements.winds,
        &mut measurements.maximum_gusts,
        &mut measurements.maximum_gust_keys,
        winds,
        maximum_gusts,
        date,
        municipality_id,
    )?;
    insert_hourly_relative_humidity(
        &mut measurements.relative_humidity,
        &mut measurements.relative_humidity_keys,
        relative_humidity,
        date,
        municipality_id,
    )?;
    insert_hourly_apparent_temperatures(
        &mut measurements.apparent_temperatures,
        apparent_temperatures,
        date,
        municipality_id,
    )
}

fn insert_hourly_wind_and_maximum_gusts(
    wind_index: &mut BTreeMap<(String, u8), NormalizedWind>,
    maximum_gusts: &mut BTreeMap<(String, u8), u16>,
    maximum_gust_keys: &mut HashSet<(String, u8)>,
    source_winds: Vec<ForecastWind>,
    maximum_gust_values: Vec<ForecastPeriodValue>,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    for value in source_winds {
        let hour = normalize_measurement_hour(&value.period, "wind", municipality_id)?;
        let direction = normalize_wind_direction(value.directions, municipality_id)?;
        let speed_kilometres_per_hour =
            normalize_single_u16(value.speeds, "wind speed", municipality_id)?;
        let key = (date.to_owned(), hour);
        if wind_index
            .insert(
                key,
                NormalizedWind {
                    direction,
                    speed_kilometres_per_hour,
                },
            )
            .is_some()
        {
            bail!("forecast {municipality_id} contains duplicate wind hours");
        }
    }
    for value in maximum_gust_values {
        let period = value
            .period
            .as_deref()
            .map(str::trim)
            .context("hourly maximum gust does not contain a period")?;
        let hour = normalize_measurement_hour(period, "maximum gust", municipality_id)?;
        let key = (date.to_owned(), hour);
        if !maximum_gust_keys.insert(key.clone()) {
            bail!("forecast {municipality_id} contains duplicate maximum gust hours");
        }
        let Some(gust) = normalize_optional_u16(value.value, "maximum gust", municipality_id)?
        else {
            continue;
        };
        maximum_gusts.insert(key, gust);
    }
    Ok(())
}

fn insert_hourly_relative_humidity(
    humidity: &mut BTreeMap<(String, u8), u8>,
    humidity_keys: &mut HashSet<(String, u8)>,
    values: Vec<ForecastPeriodValue>,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    for value in values {
        let period = value
            .period
            .as_deref()
            .map(str::trim)
            .context("hourly relative humidity does not contain a period")?;
        let hour = normalize_measurement_hour(period, "relative humidity", municipality_id)?;
        let key = (date.to_owned(), hour);
        if !humidity_keys.insert(key.clone()) {
            bail!("forecast {municipality_id} contains duplicate relative humidity hours");
        }
        let Some(percent) =
            normalize_optional_u16(value.value, "relative humidity", municipality_id)?
        else {
            continue;
        };
        let percent = u8::try_from(percent)
            .ok()
            .filter(|percent| *percent <= 100)
            .context("relative humidity is outside the supported range")?;
        humidity.insert(key, percent);
    }
    Ok(())
}

fn insert_hourly_apparent_temperatures(
    temperatures: &mut BTreeMap<(String, u8), i16>,
    values: Vec<ForecastPeriodValue>,
    date: &str,
    municipality_id: &str,
) -> Result<()> {
    for value in values {
        let period = value
            .period
            .as_deref()
            .map(str::trim)
            .context("hourly apparent temperature does not contain a period")?;
        let hour = normalize_measurement_hour(period, "apparent temperature", municipality_id)?;
        let Some(source) = value.value else {
            continue;
        };
        if source.is_empty_text() {
            continue;
        }
        let celsius = source.signed_integer().with_context(|| {
            format!("invalid apparent temperature in forecast {municipality_id}")
        })?;
        if temperatures
            .insert((date.to_owned(), hour), celsius)
            .is_some()
        {
            bail!("forecast {municipality_id} contains duplicate apparent temperature hours");
        }
    }
    Ok(())
}

fn normalize_measurement_hour(period: &str, label: &str, municipality_id: &str) -> Result<u8> {
    let hour = period
        .trim()
        .parse::<u8>()
        .with_context(|| format!("invalid {label} hour in forecast {municipality_id}"))?;
    if hour > 23 {
        bail!("invalid {label} hour in forecast {municipality_id}: {hour}");
    }
    Ok(hour)
}

fn normalize_wind_direction(
    values: Vec<SourceValue>,
    municipality_id: &str,
) -> Result<Option<String>> {
    let Some(value) = normalize_single_source_value(values, "wind direction", municipality_id)?
    else {
        return Ok(None);
    };
    let SourceValue::Text(direction) = value else {
        bail!("invalid wind direction in forecast {municipality_id}");
    };
    let direction = direction.trim().to_uppercase();
    if direction.is_empty() {
        return Ok(None);
    }
    if !matches!(
        direction.as_str(),
        "C" | "N" | "NE" | "E" | "SE" | "S" | "SO" | "O" | "NO" | "VRB"
    ) {
        bail!("invalid wind direction in forecast {municipality_id}: {direction}");
    }
    Ok(Some(direction))
}

fn normalize_single_u16(
    values: Vec<SourceValue>,
    label: &str,
    municipality_id: &str,
) -> Result<Option<u16>> {
    let value = normalize_single_source_value(values, label, municipality_id)?;
    normalize_optional_u16(value, label, municipality_id)
}

fn normalize_single_source_value(
    mut values: Vec<SourceValue>,
    label: &str,
    municipality_id: &str,
) -> Result<Option<SourceValue>> {
    if values.len() > 1 {
        bail!("forecast {municipality_id} contains multiple {label} values for one hour");
    }
    Ok(values.pop())
}

fn normalize_optional_u16(
    value: Option<SourceValue>,
    label: &str,
    municipality_id: &str,
) -> Result<Option<u16>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty_text() {
        return Ok(None);
    }
    value
        .non_negative_integer()
        .map(Some)
        .with_context(|| format!("invalid {label} in forecast {municipality_id}"))
}

fn join_hourly_forecasts(
    mut daily_forecasts: BTreeMap<String, DailyForecast>,
    conditions: &BTreeMap<(String, u8), NormalizedCondition>,
    measurements: &HourlyMeasurementIndex,
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
        let wind = measurements.winds.get(&key);
        daily_forecast.hourly_forecasts.push(HourlyForecast {
            hour,
            temperature_celsius,
            condition: condition.condition,
            description: condition.description.clone(),
            precipitation_amount: measurements.precipitation_amounts.get(&key).copied(),
            wind_direction: wind.and_then(|value| value.direction.clone()),
            wind_speed_kilometres_per_hour: wind.and_then(|value| value.speed_kilometres_per_hour),
            maximum_gust_kilometres_per_hour: measurements.maximum_gusts.get(&key).copied(),
            relative_humidity_percent: measurements.relative_humidity.get(&key).copied(),
            apparent_temperature_celsius: measurements.apparent_temperatures.get(&key).copied(),
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
    precipitation_probabilities: Vec<PrecipitationProbability>,
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
        precipitation_probabilities,
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
