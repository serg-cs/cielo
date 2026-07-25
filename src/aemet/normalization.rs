use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{Cursor, Read},
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use tracing::warn;

use super::decoding::{decode_iso_8859_15, repair_iso_8859_15_mojibake};
use super::models::{HourlyForecast, MunicipalityForecast, WeatherCondition};

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
struct Prediction {
    #[serde(rename = "dia")]
    days: Vec<ForecastDay>,
}

#[derive(Debug, Deserialize)]
struct ForecastDay {
    #[serde(rename = "fecha")]
    date: String,
    #[serde(default, rename = "estado_cielo")]
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
}

fn deserialize_one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
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
    let mut temperature_values = Vec::new();
    for day in root.prediction.days {
        validate_date(&day.date)?;
        for value in day.sky_states {
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

            let key = (day.date.clone(), hour);
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

        for value in day.temperatures {
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
            temperature_values.push((day.date.clone(), hour, celsius));
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

    temperature_values.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    if temperature_values
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1)
    {
        bail!("forecast {} contains duplicate temperature hours", root.id);
    }

    // Prefer exact or earlier conditions, falling forward only before the first state.
    let mut hourly_forecasts = Vec::with_capacity(temperature_values.len());
    for (date, hour, temperature_celsius) in temperature_values {
        let key = (date.clone(), hour);
        let condition = condition_at_or_near(&conditions, &key)
            .context("forecast condition index unexpectedly became empty")?;
        hourly_forecasts.push(HourlyForecast {
            date,
            hour,
            temperature_celsius,
            condition: condition.condition,
            description: condition.description.clone(),
        });
    }

    Ok(Some(MunicipalityForecast {
        id: root.id,
        name: repair_iso_8859_15_mojibake(&root.name),
        province: repair_iso_8859_15_mojibake(&root.province),
        generated_at: root.generated_at,
        hourly_forecasts,
    }))
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

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn u64_to_usize(value: u64) -> Result<usize> {
    usize::try_from(value).context("archive entry does not fit in memory")
}
