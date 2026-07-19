use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use reqwest::{Client, StatusCode, Url, header::RETRY_AFTER, redirect::Policy};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use tokio::time::sleep;
use tracing::warn;

#[cfg(test)]
mod tests;

const API_BASE_URL: &str = "https://opendata.aemet.es/opendata/";
const FORECAST_ENDPOINT: &str = "api/prediccion/especifica/municipio/horaria/todos";
const MUNICIPALITIES_ENDPOINT: &str = "api/maestro/municipios";
const MAX_ARCHIVE_ENTRY_SIZE: u64 = 1024 * 1024;
const MAX_ATTEMPTS: usize = 4;
const MAX_DECOMPRESSED_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_ENVELOPE_SIZE: usize = 64 * 1024;
const MAX_FORECAST_ARCHIVE_SIZE: usize = 16 * 1024 * 1024;
const MAX_FORECASTS: usize = 10_000;
const MAX_MUNICIPALITIES_SIZE: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Clone, Copy)]
enum RequestKind {
    Api,
    Data,
}

impl RequestKind {
    fn is_authenticated(self) -> bool {
        matches!(self, Self::Api)
    }

    fn has_sensitive_path(self) -> bool {
        matches!(self, Self::Data)
    }
}

/// Complete source data needed to generate one snapshot.
#[derive(Debug)]
pub(crate) struct AemetData {
    pub(crate) municipalities: HashMap<String, String>,
    pub(crate) forecasts: Vec<Forecast>,
}

/// Normalized hourly forecast from one archive member.
#[derive(Debug)]
pub(crate) struct Forecast {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) province: String,
    pub(crate) generated_at: String,
    pub(crate) temperatures: Vec<Temperature>,
}

/// One source-shaped AEMET temperature value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Temperature {
    pub(crate) date: String,
    pub(crate) hour: u8,
    pub(crate) celsius: i16,
}

/// HTTP adapter for AEMET's two-step `OpenData` protocol.
pub(crate) struct AemetClient {
    api_key: String,
    base_url: Url,
    client: Client,
    retry_base_delay: Duration,
}

impl AemetClient {
    /// Build a production client for the official AEMET service.
    pub(crate) fn new(api_key: String) -> Result<Self> {
        let base_url = Url::parse(API_BASE_URL).context("invalid built-in AEMET base URL")?;
        Self::with_base_url(api_key, base_url, Duration::from_secs(1))
    }

    fn with_base_url(api_key: String, base_url: Url, retry_base_delay: Duration) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("cielo/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build AEMET HTTP client")?;

        Ok(Self {
            api_key,
            base_url,
            client,
            retry_base_delay,
        })
    }

    /// Fetch and normalize the municipality and hourly forecast products.
    pub(crate) async fn fetch(&self) -> Result<AemetData> {
        // Resolve and download both independent products concurrently.
        let (municipality_bytes, forecast_bytes) = tokio::try_join!(
            self.fetch_product(MUNICIPALITIES_ENDPOINT, MAX_MUNICIPALITIES_SIZE),
            self.fetch_product(FORECAST_ENDPOINT, MAX_FORECAST_ARCHIVE_SIZE),
        )?;

        // Normalize AEMET's two different text encodings into domain values.
        let municipalities = parse_municipalities(&municipality_bytes)
            .context("failed to parse AEMET municipalities")?;
        let forecasts = parse_forecast_archive(&forecast_bytes)
            .context("failed to parse AEMET hourly forecast archive")?;

        Ok(AemetData {
            municipalities,
            forecasts,
        })
    }

    async fn fetch_product(&self, endpoint: &str, max_size: usize) -> Result<Vec<u8>> {
        let endpoint_url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("invalid AEMET endpoint: {endpoint}"))?;
        for attempt in 0..MAX_ATTEMPTS {
            let envelope_bytes = self
                .get_bytes(endpoint_url.clone(), RequestKind::Api, MAX_ENVELOPE_SIZE)
                .await?;
            let envelope: ApiEnvelope = serde_json::from_slice(&envelope_bytes)
                .context("AEMET returned an invalid response envelope")?;

            if envelope.status == 200 {
                let data_location = envelope
                    .data
                    .context("successful AEMET response did not include a data URL")?;
                let data_url =
                    Url::parse(&data_location).context("AEMET returned an invalid data URL")?;
                self.validate_data_url(&data_url)?;
                return self.get_bytes(data_url, RequestKind::Data, max_size).await;
            }

            if is_retryable_code(envelope.status) && attempt + 1 < MAX_ATTEMPTS {
                let reason = format!("AEMET envelope status {}", envelope.status);
                self.wait_before_retry(attempt, None, &endpoint_url, RequestKind::Api, &reason)
                    .await;
                continue;
            }

            bail!(
                "AEMET request failed with status {}: {}",
                envelope.status,
                envelope.description
            );
        }

        bail!("AEMET request exhausted all retries")
    }

    async fn get_bytes(
        &self,
        url: Url,
        request_kind: RequestKind,
        max_size: usize,
    ) -> Result<Vec<u8>> {
        'attempts: for attempt in 0..MAX_ATTEMPTS {
            let mut request = self.client.get(url.clone());
            if request_kind.is_authenticated() {
                request = request.header("api_key", &self.api_key);
            }

            let mut response = match request.send().await {
                Ok(response) => response,
                Err(error) if attempt + 1 < MAX_ATTEMPTS => {
                    let reason = error.without_url().to_string();
                    self.wait_before_retry(attempt, None, &url, request_kind, &reason)
                        .await;
                    continue;
                }
                Err(error) => {
                    return Err(error.without_url()).with_context(|| {
                        format!("request to {} failed", redact_url(&url, request_kind))
                    });
                }
            };

            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);

            if is_retryable(status) && attempt + 1 < MAX_ATTEMPTS {
                self.wait_before_retry(attempt, retry_after, &url, request_kind, status.as_str())
                    .await;
                continue;
            }
            if !status.is_success() {
                bail!(
                    "request to {} failed with HTTP status {status}",
                    redact_url(&url, request_kind)
                );
            }
            if response
                .content_length()
                .is_some_and(|length| length > usize_to_u64(max_size))
            {
                bail!(
                    "response from {} is too large",
                    redact_url(&url, request_kind)
                );
            }

            let initial_capacity = response
                .content_length()
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or_default()
                .min(max_size);
            let mut body = Vec::with_capacity(initial_capacity);
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) if chunk.len() <= max_size.saturating_sub(body.len()) => {
                        body.extend_from_slice(&chunk);
                    }
                    Ok(Some(_)) => bail!(
                        "response from {} is too large",
                        redact_url(&url, request_kind)
                    ),
                    Ok(None) => return Ok(body),
                    Err(error) if attempt + 1 < MAX_ATTEMPTS => {
                        let reason = error.without_url().to_string();
                        self.wait_before_retry(attempt, None, &url, request_kind, &reason)
                            .await;
                        continue 'attempts;
                    }
                    Err(error) => {
                        return Err(error.without_url()).with_context(|| {
                            format!(
                                "failed to read response from {}",
                                redact_url(&url, request_kind)
                            )
                        });
                    }
                }
            }
        }

        bail!(
            "request to {} exhausted all retries",
            redact_url(&url, request_kind)
        )
    }

    async fn wait_before_retry(
        &self,
        attempt: usize,
        retry_after: Option<Duration>,
        url: &Url,
        request_kind: RequestKind,
        reason: &str,
    ) {
        let shift = u32::try_from(attempt).unwrap_or(u32::MAX);
        let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
        let delay = retry_after.unwrap_or_else(|| self.retry_base_delay.saturating_mul(multiplier));
        warn!(
            attempt = attempt + 1,
            delay_seconds = delay.as_secs_f64(),
            url = %redact_url(url, request_kind),
            reason,
            "retrying AEMET request"
        );
        sleep(delay).await;
    }

    fn validate_data_url(&self, data_url: &Url) -> Result<()> {
        let same_origin = data_url.scheme() == self.base_url.scheme()
            && data_url.host_str() == self.base_url.host_str()
            && data_url.port_or_known_default() == self.base_url.port_or_known_default();
        if !same_origin {
            bail!("AEMET returned an untrusted data URL");
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    #[serde(rename = "estado")]
    status: u16,
    #[serde(rename = "descripcion")]
    description: String,
    #[serde(rename = "datos")]
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MunicipalityRecord {
    id: String,
    #[serde(rename = "nombre")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ForecastDocument {
    root: ForecastRoot,
}

#[derive(Debug, Deserialize)]
struct ForecastRoot {
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

fn parse_municipalities(bytes: &[u8]) -> Result<HashMap<String, String>> {
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

fn parse_forecast_archive(bytes: &[u8]) -> Result<Vec<Forecast>> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut forecasts = Vec::new();
    let mut ids = HashSet::new();
    let mut total_size = 0_u64;

    for entry in archive.entries().context("invalid tar archive")? {
        if forecasts.len() >= MAX_FORECASTS {
            bail!("forecast archive contains too many entries");
        }
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

        if !ids.insert(forecast.id.clone()) {
            bail!("duplicate forecast ID in archive: {}", forecast.id);
        }
        forecasts.push(forecast);
    }

    if forecasts.is_empty() {
        bail!("forecast archive is empty");
    }

    Ok(forecasts)
}

fn normalize_forecast(root: ForecastRoot, filename_id: &str) -> Result<Forecast> {
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

    let mut temperatures = Vec::new();
    for day in root.prediction.days {
        validate_date(&day.date)?;
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
            temperatures.push(Temperature {
                date: day.date.clone(),
                hour,
                celsius,
            });
        }
    }
    if temperatures.is_empty() {
        bail!("forecast {} does not contain temperatures", root.id);
    }

    temperatures.sort_by(|left, right| (&left.date, left.hour).cmp(&(&right.date, right.hour)));
    if temperatures
        .windows(2)
        .any(|pair| pair[0].date == pair[1].date && pair[0].hour == pair[1].hour)
    {
        bail!("forecast {} contains duplicate temperature hours", root.id);
    }

    Ok(Forecast {
        id: root.id,
        name: repair_iso_8859_15_mojibake(&root.name),
        province: repair_iso_8859_15_mojibake(&root.province),
        generated_at: root.generated_at,
        temperatures,
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

fn decode_iso_8859_15(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0xA4 => '\u{20AC}',
            0xA6 => '\u{0160}',
            0xA8 => '\u{0161}',
            0xB4 => '\u{017D}',
            0xB8 => '\u{017E}',
            0xBC => '\u{0152}',
            0xBD => '\u{0153}',
            0xBE => '\u{0178}',
            value => char::from(*value),
        })
        .collect()
}

fn repair_iso_8859_15_mojibake(value: &str) -> String {
    let Some(bytes) = encode_iso_8859_15(value) else {
        return value.to_owned();
    };

    String::from_utf8(bytes).unwrap_or_else(|_| value.to_owned())
}

fn encode_iso_8859_15(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(value.len());
    for character in value.chars() {
        let byte = match character {
            '\u{20AC}' => 0xA4,
            '\u{0160}' => 0xA6,
            '\u{0161}' => 0xA8,
            '\u{017D}' => 0xB4,
            '\u{017E}' => 0xB8,
            '\u{0152}' => 0xBC,
            '\u{0153}' => 0xBD,
            '\u{0178}' => 0xBE,
            '\u{00A4}' | '\u{00A6}' | '\u{00A8}' | '\u{00B4}' | '\u{00B8}' | '\u{00BC}'
            | '\u{00BD}' | '\u{00BE}' => return None,
            _ => u8::try_from(u32::from(character)).ok()?,
        };
        bytes.push(byte);
    }
    Some(bytes)
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_code(status: u16) -> bool {
    StatusCode::from_u16(status).is_ok_and(is_retryable)
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    Some(
        retry_at
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO),
    )
}

fn redact_url(url: &Url, request_kind: RequestKind) -> String {
    let mut redacted = url.clone();
    if request_kind.has_sensitive_path() {
        redacted.set_path("/redacted");
    }
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.to_string()
}

fn u64_to_usize(value: u64) -> Result<usize> {
    usize::try_from(value).context("archive entry does not fit in memory")
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
