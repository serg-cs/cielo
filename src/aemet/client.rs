use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use reqwest::{Client, StatusCode, Url, header::RETRY_AFTER, redirect::Policy};
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
    time::{Instant, sleep},
};
use tracing::{debug, warn};

use crate::catalog::{Beach, Municipality};

use super::{
    archive::{MAX_DOCUMENT_ENTRY, file_name, read_archive},
    beaches::parse_beach,
    fire::{FireForecast, parse_fire_archive},
    forecast::{decode_document, id_from_file_name, parse_daily, parse_hourly},
    models::{
        AemetProduct, BeachForecast, DailyForecast, EntryFailure, HourlyForecast, ParsedArchive,
        WeatherWarning,
    },
    sources::{parse_beach_list, parse_master_list},
    warnings::{CapMessage, active_warnings, parse_cap, parse_warning_archive, rss_links},
};

const API_BASE_URL: &str = "https://opendata.aemet.es/opendata/";
const PUBLIC_BASE_URL: &str = "https://www.aemet.es/";
const DAILY_FORECAST_ENDPOINT: &str = "api/prediccion/especifica/municipio/diaria/todos";
const HOURLY_FORECAST_ENDPOINT: &str = "api/prediccion/especifica/municipio/horaria/todos";
const MUNICIPALITIES_ENDPOINT: &str = "api/maestro/municipios";
const WARNINGS_ENDPOINT: &str = "api/avisos_cap/ultimoelaborado/area/esp";
const FIRE_PATH: &str = "es/api-eltiempo/incendios/download";
const WARNINGS_RSS_PATH: &str = "documentos_d/eltiempo/prediccion/avisos/rss/CAP_AFAE_wah_RSS.xml";
const BEACH_PATH: &str = "xml/playas/play_v2_";
const BEACH_LIST_PATH: &str = "documentos/es/eltiempo/prediccion/playas/Playas_codigos.csv";

pub(super) const MAX_ATTEMPTS: usize = 4;
/// Waits before each fresh attempt at a whole product, as multiples of the
/// retry base delay (one second): 15 s, 45 s and 90 s, about 2.5 minutes.
const PRODUCT_RETRY_FACTORS: [u32; 3] = [15, 45, 90];
/// Attempts at a whole product: the first one and one after each wait.
#[cfg(test)]
pub(super) const PRODUCT_ATTEMPTS: usize = PRODUCT_RETRY_FACTORS.len() + 1;
const MAX_ENVELOPE_SIZE: usize = 64 * 1024;
const MAX_ARCHIVE_SIZE: usize = 32 * 1024 * 1024;
const MAX_DOCUMENT_SIZE: usize = 1024 * 1024;
const MAX_FIRE_SIZE: usize = 16 * 1024 * 1024;
const RATE_LIMIT_RETRY_BASE_DELAY: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);
/// Spacing between authenticated API calls; AEMET allows about 40 a minute.
const API_PACE: Duration = Duration::from_millis(1700);
/// Concurrent downloads from the public `www.aemet.es` site.
const PUBLIC_CONCURRENCY: usize = 6;
/// Rejected-key answers in a row before giving up on per-province calls.

#[derive(Clone, Copy)]
pub(super) enum RequestKind {
    Api,
    Data,
    Public,
}

impl RequestKind {
    fn is_authenticated(self) -> bool {
        matches!(self, Self::Api)
    }

    fn has_sensitive_path(self) -> bool {
        matches!(self, Self::Data)
    }
}

#[derive(Clone, Copy)]
pub(super) enum RetryKind {
    Transient,
    RateLimited,
}

/// HTTP adapter for AEMET's two-step `OpenData` protocol and its public files.
#[derive(Clone)]
pub(crate) struct AemetClient {
    api_key: Option<String>,
    base_url: Url,
    public_base_url: Url,
    client: Client,
    retry_base_delay: Duration,
    pace: Duration,
    last_api_call: Arc<Mutex<Option<Instant>>>,
    public_slots: Arc<Semaphore>,
    dump_directory: Option<PathBuf>,
}

impl AemetClient {
    /// Build a production client. Without a key only public files can be fetched.
    pub(crate) fn new(api_key: Option<String>, dump_directory: Option<PathBuf>) -> Result<Self> {
        let base_url = Url::parse(API_BASE_URL).context("invalid built-in AEMET base URL")?;
        let public_base_url =
            Url::parse(PUBLIC_BASE_URL).context("invalid built-in AEMET public URL")?;
        let mut client = Self::with_base_urls(
            api_key,
            base_url,
            public_base_url,
            Duration::from_secs(1),
            API_PACE,
        )?;
        client.dump_directory = dump_directory;
        Ok(client)
    }

    pub(super) fn with_base_urls(
        api_key: Option<String>,
        base_url: Url,
        public_base_url: Url,
        retry_base_delay: Duration,
        pace: Duration,
    ) -> Result<Self> {
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
            public_base_url,
            client,
            retry_base_delay,
            pace,
            last_api_call: Arc::new(Mutex::new(None)),
            public_slots: Arc::new(Semaphore::new(PUBLIC_CONCURRENCY)),
            dump_directory: None,
        })
    }

    #[cfg(test)]
    pub(super) fn with_base_url(
        api_key: String,
        base_url: Url,
        retry_base_delay: Duration,
    ) -> Result<Self> {
        let public = base_url.clone();
        Self::with_base_urls(
            Some(api_key),
            base_url,
            public,
            retry_base_delay,
            Duration::ZERO,
        )
    }

    pub(crate) fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// AEMET's municipality master list, to refresh the catalog.
    pub(crate) async fn municipality_list(&self) -> Result<Vec<Municipality>> {
        self.fetch_parsed_product(MUNICIPALITIES_ENDPOINT, MAX_ARCHIVE_SIZE, parse_master_list)
            .await?
            .with_context(|| format!("AEMET has no data for {MUNICIPALITIES_ENDPOINT}"))
    }

    /// AEMET's list of beaches with a forecast, to refresh the catalog.
    pub(crate) async fn beach_list(&self) -> Result<Vec<Beach>> {
        let bytes = self.get_public(BEACH_LIST_PATH, MAX_DOCUMENT_SIZE).await?;
        parse_beach_list(&bytes).context("failed to parse AEMET's beach list")
    }

    /// Hourly forecasts for every municipality.
    pub(crate) async fn hourly_forecasts(&self) -> Result<ParsedArchive<HourlyForecast>> {
        self.fetch_parsed_product(HOURLY_FORECAST_ENDPOINT, MAX_ARCHIVE_SIZE, |bytes| {
            self.dump("horaria.tar.gz", bytes);
            parse_forecast_archive(bytes, AemetProduct::Hourly, "localidad_h_", parse_hourly)
                .context("failed to parse AEMET hourly forecast archive")
        })
        .await?
        .with_context(|| format!("AEMET has no data for {HOURLY_FORECAST_ENDPOINT}"))
    }

    /// Daily forecasts for every municipality.
    pub(crate) async fn daily_forecasts(&self) -> Result<ParsedArchive<DailyForecast>> {
        self.fetch_parsed_product(DAILY_FORECAST_ENDPOINT, MAX_ARCHIVE_SIZE, |bytes| {
            self.dump("diaria.tar.gz", bytes);
            parse_forecast_archive(bytes, AemetProduct::Daily, "localidad_", parse_daily)
                .context("failed to parse AEMET daily forecast archive")
        })
        .await?
        .with_context(|| format!("AEMET has no data for {DAILY_FORECAST_ENDPOINT}"))
    }

    /// Active weather warnings, from the API or, failing that, the public feed.
    pub(crate) async fn warnings(
        &self,
        now: Timestamp,
    ) -> Result<(Vec<WeatherWarning>, Vec<EntryFailure>)> {
        if self.has_api_key() {
            match self
                .fetch_parsed_product(WARNINGS_ENDPOINT, MAX_ARCHIVE_SIZE, |bytes| {
                    self.dump("avisos.tar", bytes);
                    parse_warning_archive(bytes, now)
                        .context("failed to parse AEMET warnings archive")
                })
                .await
            {
                Ok(Some(warnings)) => return Ok(warnings),
                Ok(None) => return Ok((Vec::new(), Vec::new())),
                Err(error) => warn!(
                    error = format!("{error:#}"),
                    "AEMET warnings API failed; using the public feed"
                ),
            }
        }
        self.public_warnings(now).await
    }

    async fn public_warnings(
        &self,
        now: Timestamp,
    ) -> Result<(Vec<WeatherWarning>, Vec<EntryFailure>)> {
        let feed = self
            .get_public(WARNINGS_RSS_PATH, MAX_DOCUMENT_SIZE)
            .await
            .context("failed to fetch the public warnings feed")?;
        let links = rss_links(&decode_document(&feed))?;
        let mut tasks = JoinSet::new();
        for link in links {
            let client = self.clone();
            tasks.spawn(async move {
                let result = async {
                    let url = Url::parse(&link).context("invalid warning link")?;
                    let bytes = client.get_public(url.path(), MAX_DOCUMENT_SIZE).await?;
                    parse_cap(&decode_document(&bytes))
                }
                .await;
                (link, result)
            });
        }
        let mut messages: Vec<CapMessage> = Vec::new();
        let mut failures = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let Ok((link, result)) = result else {
                continue;
            };
            match result {
                Ok(message) => messages.push(message),
                Err(error) => failures.push(EntryFailure {
                    id: file_name(&link).to_owned(),
                    product: AemetProduct::Warnings,
                    error,
                }),
            }
        }
        Ok((active_warnings(messages, now), failures))
    }

    /// Wildfire danger rasters (public download).
    pub(crate) async fn fire(&self) -> Result<FireForecast> {
        let bytes = self.get_public(FIRE_PATH, MAX_FIRE_SIZE).await?;
        self.dump("incendios.tar.gz", &bytes);
        parse_fire_archive(&bytes).context("failed to parse AEMET fire danger download")
    }

    /// Beach forecasts (public XML, one file per beach).
    pub(crate) async fn beaches(
        &self,
        beaches: &[Beach],
    ) -> (Vec<BeachForecast>, Vec<EntryFailure>) {
        let mut tasks = JoinSet::new();
        for beach in beaches {
            let client = self.clone();
            let beach = beach.clone();
            tasks.spawn(async move {
                let result = async {
                    let bytes = client
                        .get_public(&format!("{BEACH_PATH}{}.xml", beach.id), MAX_DOCUMENT_SIZE)
                        .await?;
                    parse_beach(&decode_document(&bytes), &beach)
                }
                .await;
                (beach.id, result)
            });
        }
        let (mut forecasts, mut failures) = (Vec::new(), Vec::new());
        while let Some(result) = tasks.join_next().await {
            let Ok((id, result)) = result else {
                continue;
            };
            match result {
                Ok(forecast) => forecasts.push(forecast),
                Err(error) => failures.push(EntryFailure {
                    id,
                    product: AemetProduct::Beaches,
                    error,
                }),
            }
        }
        forecasts.sort_by(|left, right| left.id.cmp(&right.id));
        (forecasts, failures)
    }

    fn dump(&self, name: &str, bytes: &[u8]) {
        let Some(directory) = &self.dump_directory else {
            return;
        };
        let result = std::fs::create_dir_all(directory)
            .and_then(|()| std::fs::write(directory.join(name), bytes));
        if let Err(error) = result {
            warn!(%error, name, "failed to save AEMET source payload");
        }
    }

    /// Fetch an API product; AEMET's "no data" answer is an error.
    #[cfg(test)]
    pub(super) async fn fetch_product(&self, endpoint: &str, max_size: usize) -> Result<Vec<u8>> {
        self.fetch_parsed_product(endpoint, max_size, |bytes| Ok(bytes.to_vec()))
            .await?
            .with_context(|| format!("AEMET has no data for {endpoint}"))
    }

    /// Fetch an API product and parse it; AEMET's "no data" answer is `None`.
    ///
    /// AEMET fails often and briefly: a 5xx or 429 answer, a page that is not
    /// an API envelope, a data link that fails or expires, or a truncated
    /// archive. Any of these starts a fresh attempt (new request, download and
    /// parse) after a growing wait, so a short outage does not lose the run.
    /// Only a rejected API key fails at once, since retrying cannot fix it.
    pub(super) async fn fetch_parsed_product<T>(
        &self,
        endpoint: &str,
        max_size: usize,
        parse: impl Fn(&[u8]) -> Result<T>,
    ) -> Result<Option<T>> {
        if self.api_key.is_none() {
            bail!("AEMET_API_KEY is required for {endpoint}");
        }
        let endpoint_url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("invalid AEMET endpoint: {endpoint}"))?;
        let mut attempt = 0;
        loop {
            let result = match self.fetch_product_once(&endpoint_url, max_size).await {
                Ok(Some(bytes)) => parse(&bytes).map(Some),
                Ok(None) => Ok(None),
                Err(error) => Err(error),
            };
            match result {
                Ok(value) => return Ok(value),
                Err(error) if !is_permanent(&error) && attempt < PRODUCT_RETRY_FACTORS.len() => {
                    self.wait_before_product_retry(attempt, endpoint, &error)
                        .await;
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// One request for a product's envelope and the download it points to.
    async fn fetch_product_once(
        &self,
        endpoint_url: &Url,
        max_size: usize,
    ) -> Result<Option<Vec<u8>>> {
        self.wait_for_pace().await;
        let (status, envelope_bytes) = self
            .get_response(endpoint_url.clone(), RequestKind::Api, MAX_ENVELOPE_SIZE)
            .await?;
        let envelope = match serde_json::from_slice::<ApiEnvelope>(&envelope_bytes) {
            Ok(envelope) => envelope,
            Err(_) if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => {
                bail!(KeyError(format!("HTTP status {status}")))
            }
            Err(_) if !status.is_success() => bail!(
                "request to {} failed with HTTP status {status}",
                redact_url(endpoint_url, RequestKind::Api)
            ),
            Err(error) => {
                return Err(error).context("AEMET returned an invalid response envelope");
            }
        };
        match envelope.status {
            200 => {
                let data_location = envelope
                    .data
                    .context("successful AEMET response did not include a data URL")?;
                let data_url =
                    Url::parse(&data_location).context("AEMET returned an invalid data URL")?;
                self.validate_data_url(&data_url)?;
                let data = self
                    .get_bytes(data_url, RequestKind::Data, max_size)
                    .await?;
                if is_expired_data(&data) {
                    bail!("AEMET data URL expired before it was downloaded");
                }
                Ok(Some(data))
            }
            404 => Ok(None),
            401 | 403 => bail!(KeyError(envelope.description)),
            code => bail!(
                "AEMET request failed with status {code}: {}",
                envelope.description
            ),
        }
    }

    async fn wait_before_product_retry(&self, attempt: usize, what: &str, error: &anyhow::Error) {
        let delay = self
            .retry_base_delay
            .saturating_mul(PRODUCT_RETRY_FACTORS[attempt]);
        warn!(
            attempt = attempt + 1,
            delay_seconds = delay.as_secs_f64(),
            product = what,
            error = format!("{error:#}"),
            "AEMET product failed; trying again"
        );
        sleep(delay).await;
    }

    /// Fetch a file from the public `www.aemet.es` site.
    pub(super) async fn get_public(&self, path: &str, max_size: usize) -> Result<Vec<u8>> {
        let url = self
            .public_base_url
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("invalid AEMET public path: {path}"))?;
        if url.host_str() != self.public_base_url.host_str()
            || url.scheme() != self.public_base_url.scheme()
        {
            bail!("refusing to fetch untrusted public URL");
        }
        let _slot = self
            .public_slots
            .acquire()
            .await
            .context("public download limiter closed")?;
        self.get_bytes(url, RequestKind::Public, max_size).await
    }

    async fn wait_for_pace(&self) {
        if self.pace.is_zero() {
            return;
        }
        let mut last = self.last_api_call.lock().await;
        if let Some(last_call) = *last {
            let ready = last_call + self.pace;
            if ready > Instant::now() {
                sleep(ready - Instant::now()).await;
            }
        }
        *last = Some(Instant::now());
    }

    async fn get_bytes(
        &self,
        url: Url,
        request_kind: RequestKind,
        max_size: usize,
    ) -> Result<Vec<u8>> {
        let (status, body) = self
            .get_response(url.clone(), request_kind, max_size)
            .await?;
        if !status.is_success() {
            bail!(
                "request to {} failed with HTTP status {status}",
                redact_url(&url, request_kind)
            );
        }
        Ok(body)
    }

    /// GET with retries. Client errors are returned with their body so API
    /// envelopes describing them can be read.
    #[allow(clippy::too_many_lines)]
    async fn get_response(
        &self,
        url: Url,
        request_kind: RequestKind,
        max_size: usize,
    ) -> Result<(StatusCode, Vec<u8>)> {
        'attempts: for attempt in 0..MAX_ATTEMPTS {
            let mut request = self.client.get(url.clone());
            if request_kind.is_authenticated()
                && let Some(api_key) = &self.api_key
            {
                request = request.header("api_key", api_key);
            }

            let mut response = match request.send().await {
                Ok(response) => response,
                Err(error) if attempt + 1 < MAX_ATTEMPTS => {
                    let reason = error.without_url().to_string();
                    self.wait_before_retry(
                        attempt,
                        None,
                        RetryKind::Transient,
                        &url,
                        request_kind,
                        &reason,
                    )
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
            if let Some(remaining) = response.headers().get("remaining-request-endpoint") {
                debug!(remaining = ?remaining, "AEMET request quota");
            }
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);

            if is_retryable(status) && attempt + 1 < MAX_ATTEMPTS {
                self.wait_before_status_retry(attempt, retry_after, status, &url, request_kind)
                    .await;
                continue;
            }
            if status.is_redirection() {
                bail!(Permanent(format!(
                    "request to {} failed with HTTP status {status}",
                    redact_url(&url, request_kind)
                )));
            }
            if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
                bail!(
                    "request to {} failed with HTTP status {status}",
                    redact_url(&url, request_kind)
                );
            }
            if response
                .content_length()
                .is_some_and(|length| length > usize_to_u64(max_size))
            {
                bail!(Permanent(format!(
                    "response from {} is too large",
                    redact_url(&url, request_kind)
                )));
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
                    Ok(Some(_)) => bail!(Permanent(format!(
                        "response from {} is too large",
                        redact_url(&url, request_kind)
                    ))),
                    Ok(None) => return Ok((status, body)),
                    Err(error) if attempt + 1 < MAX_ATTEMPTS => {
                        let reason = error.without_url().to_string();
                        self.wait_before_retry(
                            attempt,
                            None,
                            RetryKind::Transient,
                            &url,
                            request_kind,
                            &reason,
                        )
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

    async fn wait_before_status_retry(
        &self,
        attempt: usize,
        retry_after: Option<Duration>,
        status: StatusCode,
        url: &Url,
        request_kind: RequestKind,
    ) {
        self.wait_before_retry(
            attempt,
            retry_after,
            retry_kind_for_status(status),
            url,
            request_kind,
            status.as_str(),
        )
        .await;
    }

    async fn wait_before_retry(
        &self,
        attempt: usize,
        retry_after: Option<Duration>,
        retry_kind: RetryKind,
        url: &Url,
        request_kind: RequestKind,
        reason: &str,
    ) {
        let delay = retry_delay(attempt, self.retry_base_delay, retry_after, retry_kind);
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
            bail!(Permanent("AEMET returned an untrusted data URL".to_owned()));
        }

        Ok(())
    }
}

/// A failure that another attempt cannot fix, such as an untrusted data URL,
/// a redirect or an oversized download.
#[derive(Debug)]
pub(crate) struct Permanent(String);

impl std::fmt::Display for Permanent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Permanent {}

/// Whether retrying cannot help: a rejected key or another permanent failure.
fn is_permanent(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<KeyError>() || cause.is::<Permanent>())
}

/// The API key was rejected, for example because it must be renewed.
#[derive(Debug)]
pub(crate) struct KeyError(String);

impl std::fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "AEMET rejected the API key: {}. Request or renew a key at \
             https://opendata.aemet.es/centrodedescargas/altaUsuario and update AEMET_API_KEY",
            self.0.trim()
        )
    }
}

impl std::error::Error for KeyError {}

/// Whether an error, or any of its causes, is a rejected API key.
#[cfg(test)]
pub(super) fn is_key_error(error: &anyhow::Error) -> bool {
    error.chain().any(<dyn std::error::Error>::is::<KeyError>)
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    #[serde(rename = "estado")]
    status: u16,
    #[serde(rename = "descripcion", default)]
    description: String,
    #[serde(rename = "datos")]
    data: Option<String>,
}

/// AEMET answers an expired data URL with a small "datos expirados" envelope.
fn is_expired_data(data: &[u8]) -> bool {
    data.len() < 512
        && serde_json::from_slice::<ApiEnvelope>(data)
            .is_ok_and(|envelope| envelope.status == 404 && envelope.data.is_none())
}

fn parse_forecast_archive<T>(
    bytes: &[u8],
    product: AemetProduct,
    prefix: &str,
    parse: fn(&Value, Option<&str>) -> Result<T>,
) -> Result<ParsedArchive<T>> {
    let entries = read_archive(bytes, product.as_str(), MAX_DOCUMENT_ENTRY)?;
    let mut items = Vec::new();
    let mut failures = Vec::new();
    let count = entries.len();
    for entry in entries {
        let name = file_name(&entry.path).to_owned();
        let id = id_from_file_name(&name, prefix)?;
        let result = serde_json::from_str::<Value>(&decode_document(&entry.body))
            .context("invalid JSON")
            .and_then(|value| parse(&value, Some(&id)));
        match result {
            Ok(item) => items.push(item),
            Err(error) => {
                debug!(
                    id,
                    product = product.as_str(),
                    error = format!("{error:#}"),
                    "skipping document"
                );
                failures.push(EntryFailure { id, product, error });
            }
        }
    }
    if items.is_empty() {
        match failures.first() {
            Some(failure) => bail!(
                "no usable {} forecasts; {} failed, first {}: {:#}",
                product.as_str(),
                failures.len(),
                failure.id,
                failure.error
            ),
            None => bail!("no usable {} forecasts", product.as_str()),
        }
    }
    Ok(ParsedArchive {
        items,
        failures,
        entries: count,
    })
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_kind_for_status(status: StatusCode) -> RetryKind {
    if status == StatusCode::TOO_MANY_REQUESTS {
        RetryKind::RateLimited
    } else {
        RetryKind::Transient
    }
}

pub(super) fn retry_delay(
    attempt: usize,
    retry_base_delay: Duration,
    retry_after: Option<Duration>,
    retry_kind: RetryKind,
) -> Duration {
    let shift = u32::try_from(attempt).unwrap_or(u32::MAX);
    let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
    let base_delay = match retry_kind {
        RetryKind::Transient => retry_base_delay,
        RetryKind::RateLimited => RATE_LIMIT_RETRY_BASE_DELAY,
    };
    let fallback_delay = base_delay.saturating_mul(multiplier);

    match (retry_kind, retry_after) {
        (RetryKind::RateLimited, Some(retry_after)) => fallback_delay.max(retry_after),
        (_, Some(retry_after)) => retry_after,
        (_, None) => fallback_delay,
    }
}

pub(super) fn parse_retry_after(value: &str) -> Option<Duration> {
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

pub(super) fn redact_url(url: &Url, request_kind: RequestKind) -> String {
    let mut redacted = url.clone();
    if request_kind.has_sensitive_path() {
        redacted.set_path("/redacted");
    }
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.to_string()
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
