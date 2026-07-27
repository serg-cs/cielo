mod client;
mod decoding;
mod models;
mod normalization;

#[cfg(test)]
mod tests;

pub(crate) use client::AemetClient;
pub(crate) use models::{AemetWeatherData, HourlyForecast, MunicipalityForecast, WeatherCondition};
pub(crate) use normalization::validate_municipality_id;

#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use client::{MAX_ATTEMPTS, RequestKind, RetryKind, parse_retry_after, redact_url, retry_delay};
#[cfg(test)]
use decoding::repair_iso_8859_15_mojibake;
#[cfg(test)]
use normalization::{
    ForecastDocument, normalize_forecast, parse_forecast_archive, parse_municipalities,
};
