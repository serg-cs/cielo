use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, Url, header::RETRY_AFTER, redirect::Policy};
use serde::Deserialize;
use tokio::time::sleep;
use tracing::warn;

use super::{
    AemetWeatherData,
    normalization::{parse_forecast_archive, parse_municipalities},
};

const API_BASE_URL: &str = "https://opendata.aemet.es/opendata/";
const FORECAST_ENDPOINT: &str = "api/prediccion/especifica/municipio/horaria/todos";
const MUNICIPALITIES_ENDPOINT: &str = "api/maestro/municipios";
pub(super) const MAX_ATTEMPTS: usize = 4;
const MAX_ENVELOPE_SIZE: usize = 64 * 1024;
const MAX_FORECAST_ARCHIVE_SIZE: usize = 16 * 1024 * 1024;
const MAX_MUNICIPALITIES_SIZE: usize = 8 * 1024 * 1024;
const RATE_LIMIT_RETRY_BASE_DELAY: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Clone, Copy)]
pub(super) enum RequestKind {
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

#[derive(Clone, Copy)]
pub(super) enum RetryKind {
    Transient,
    RateLimited,
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

    pub(super) fn with_base_url(
        api_key: String,
        base_url: Url,
        retry_base_delay: Duration,
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
            client,
            retry_base_delay,
        })
    }

    /// Fetch and normalize the municipality and hourly forecast products.
    pub(crate) async fn fetch(&self) -> Result<AemetWeatherData> {
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

        Ok(AemetWeatherData {
            municipalities,
            forecasts,
        })
    }

    pub(super) async fn fetch_product(&self, endpoint: &str, max_size: usize) -> Result<Vec<u8>> {
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
                let retry_kind = retry_kind_for_code(envelope.status);
                self.wait_before_retry(
                    attempt,
                    None,
                    retry_kind,
                    &endpoint_url,
                    RequestKind::Api,
                    &reason,
                )
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

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_code(status: u16) -> bool {
    StatusCode::from_u16(status).is_ok_and(is_retryable)
}

fn retry_kind_for_status(status: StatusCode) -> RetryKind {
    if status == StatusCode::TOO_MANY_REQUESTS {
        RetryKind::RateLimited
    } else {
        RetryKind::Transient
    }
}

fn retry_kind_for_code(status: u16) -> RetryKind {
    StatusCode::from_u16(status).map_or(RetryKind::Transient, retry_kind_for_status)
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
