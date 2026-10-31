use std::collections::BTreeMap;

use anyhow::{Context, Result};
use jiff::{Timestamp, Zoned, civil::Date};
use serde::{Serialize, Serializer};

/// AEMET product a failure or status refers to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum AemetProduct {
    Hourly,
    Daily,
    Warnings,
    Fire,
    Beaches,
}

impl AemetProduct {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Warnings => "warnings",
            Self::Fire => "fire",
            Self::Beaches => "beaches",
        }
    }
}

/// One document that was skipped because it could not be used.
#[derive(Debug)]
pub(crate) struct EntryFailure {
    pub(crate) id: String,
    pub(crate) product: AemetProduct,
    pub(crate) error: anyhow::Error,
}

/// Documents parsed from one archive, plus the ones that failed.
#[derive(Debug)]
pub(crate) struct ParsedArchive<T> {
    pub(crate) items: Vec<T>,
    pub(crate) failures: Vec<EntryFailure>,
    pub(crate) entries: usize,
}

/// Time zone of a municipality, decided by its province.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum MunicipalityTimeZone {
    #[serde(rename = "Africa/Ceuta")]
    AfricaCeuta,
    #[serde(rename = "Atlantic/Canary")]
    AtlanticCanary,
    #[serde(rename = "Europe/Madrid")]
    EuropeMadrid,
}

impl MunicipalityTimeZone {
    pub(crate) fn for_municipality(municipality_id: &str) -> Self {
        match municipality_id.get(..2) {
            Some("35" | "38") => Self::AtlanticCanary,
            Some("51" | "52") => Self::AfricaCeuta,
            _ => Self::EuropeMadrid,
        }
    }

    pub(crate) const fn iana_name(self) -> &'static str {
        match self {
            Self::AfricaCeuta => "Africa/Ceuta",
            Self::AtlanticCanary => "Atlantic/Canary",
            Self::EuropeMadrid => "Europe/Madrid",
        }
    }

    /// Rules for this zone from the time zone database bundled with jiff.
    pub(crate) fn rules(self) -> Result<jiff::tz::TimeZone> {
        jiff::tz::db()
            .get(self.iana_name())
            .with_context(|| format!("time zone {} is not available", self.iana_name()))
    }
}

/// Precipitation or snow accumulated during one hour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Accumulation {
    TenthsOfMillimetre(u32),
    /// AEMET's `Ip`: inapreciable, less than 0.1 mm.
    Trace,
}

impl Serialize for Accumulation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::TenthsOfMillimetre(tenths) if tenths % 10 == 0 => {
                serializer.serialize_u32(tenths / 10)
            }
            Self::TenthsOfMillimetre(tenths) => serializer.serialize_f64(f64::from(*tenths) / 10.0),
            Self::Trace => serializer.serialize_str("Ip"),
        }
    }
}

/// Values forecast for one hour. Every field is optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HourValues {
    /// AEMET sky code such as `13` or `13n`.
    pub(crate) sky: Option<String>,
    pub(crate) temperature: Option<i16>,
    pub(crate) feels_like: Option<i16>,
    pub(crate) precipitation: Option<Accumulation>,
    pub(crate) snow: Option<Accumulation>,
    pub(crate) humidity: Option<u8>,
    pub(crate) wind_direction: Option<String>,
    pub(crate) wind_speed: Option<u16>,
    pub(crate) gust: Option<u16>,
}

impl HourValues {
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Probabilities for a window of hours, such as AEMET's `0713`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProbabilityWindow {
    pub(crate) start: Zoned,
    pub(crate) end: Zoned,
    pub(crate) precipitation: Option<u8>,
    pub(crate) storm: Option<u8>,
    pub(crate) snow: Option<u8>,
}

/// Local sunrise and sunset, as `HH:MM`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SunTimes {
    pub(crate) date: Date,
    pub(crate) sunrise: Option<String>,
    pub(crate) sunset: Option<String>,
}

/// Hourly forecast product for one municipality (about 48 hours).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HourlyForecast {
    pub(crate) id: String,
    pub(crate) issued: Option<Timestamp>,
    pub(crate) hours: BTreeMap<Timestamp, HourValues>,
    pub(crate) windows: Vec<ProbabilityWindow>,
    pub(crate) sun: Vec<SunTimes>,
}

/// Values for one part of a day, such as `00-24`, `00-12` or `06-12`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PeriodValues {
    pub(crate) sky: Option<String>,
    pub(crate) precipitation_probability: Option<u8>,
    pub(crate) snow_level_metres: Option<u16>,
    pub(crate) wind_direction: Option<String>,
    pub(crate) wind_speed: Option<u16>,
    pub(crate) gust: Option<u16>,
}

impl PeriodValues {
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Minimum, maximum and values at fixed hours, such as temperature.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DailyRange {
    pub(crate) minimum: Option<i16>,
    pub(crate) maximum: Option<i16>,
    /// Values at local hours 6, 12, 18 and 24.
    pub(crate) at_hours: BTreeMap<u8, i16>,
}

/// One day of the daily forecast product.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DayForecast {
    pub(crate) date: Date,
    pub(crate) temperature: DailyRange,
    pub(crate) feels_like: DailyRange,
    pub(crate) humidity: DailyRange,
    pub(crate) uv_index: Option<u8>,
    pub(crate) periods: BTreeMap<String, PeriodValues>,
}

/// Daily forecast product for one municipality (7 days).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DailyForecast {
    pub(crate) id: String,
    pub(crate) issued: Option<Timestamp>,
    pub(crate) days: Vec<DayForecast>,
}

/// Warning level, from the Meteoalerta colour.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WarningLevel {
    Yellow,
    Orange,
    Red,
}

/// An active weather warning for one or more zones.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WeatherWarning {
    pub(crate) id: String,
    pub(crate) level: WarningLevel,
    /// Meteoalerta phenomenon code, such as `PR` or `TO`.
    pub(crate) code: Option<String>,
    /// Phenomenon in Spanish, such as `Lluvias`.
    pub(crate) name: String,
    pub(crate) onset: Option<Timestamp>,
    pub(crate) expires: Option<Timestamp>,
    pub(crate) description: Option<String>,
    /// Threshold description, such as `Precipitación acumulada en una hora: 20 mm`.
    pub(crate) parameter: Option<String>,
    pub(crate) probability: Option<String>,
    pub(crate) zones: Vec<String>,
    pub(crate) area: Option<String>,
    /// Polygons of the area as (latitude, longitude) points.
    pub(crate) polygons: Vec<Vec<(f64, f64)>>,
}

/// Wildfire danger levels (1-6) for one municipality, by date.
pub(crate) type FireDanger = BTreeMap<Date, u8>;

/// One beach forecast day: morning and afternoon values.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BeachDay {
    pub(crate) date: Date,
    pub(crate) sky: [Option<u16>; 2],
    pub(crate) wind: [Option<u16>; 2],
    pub(crate) waves: [Option<u16>; 2],
    pub(crate) maximum_temperature: Option<i16>,
    pub(crate) water_temperature: Option<i16>,
    pub(crate) feels_like: Option<u16>,
    pub(crate) uv_index: Option<u8>,
}

/// Beach forecast.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BeachForecast {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) municipality_id: String,
    pub(crate) issued: Option<Timestamp>,
    pub(crate) days: Vec<BeachDay>,
}
