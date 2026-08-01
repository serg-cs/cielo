use std::collections::HashMap;

use serde::{Serialize, Serializer};

/// Complete source data needed to generate one weather-data output.
#[derive(Debug)]
pub(crate) struct AemetWeatherData {
    pub(crate) municipalities: HashMap<String, String>,
    pub(crate) forecasts: Vec<MunicipalityForecast>,
    pub(crate) daily_forecasts: Vec<MunicipalityDailyForecast>,
}

/// Normalized hourly forecast for one municipality.
#[derive(Clone, Debug)]
pub(crate) struct MunicipalityForecast {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) province: String,
    pub(crate) generated_at: String,
    pub(crate) daily_forecasts: Vec<DailyForecast>,
}

/// Normalized daily forecast for one municipality.
#[derive(Clone, Debug)]
pub(crate) struct MunicipalityDailyForecast {
    pub(crate) id: String,
    pub(crate) generated_at: String,
    pub(crate) summaries: Vec<DailySummary>,
}

/// Daily aggregate values for one local forecast day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DailySummary {
    pub(crate) date: String,
    pub(crate) minimum_temperature_celsius: i16,
    pub(crate) maximum_temperature_celsius: i16,

    pub(crate) condition: Option<WeatherCondition>,
    pub(crate) description: Option<String>,
    pub(crate) precipitation_probability: Option<PrecipitationProbability>,
}

/// Solar times and hourly conditions for one local forecast day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DailyForecast {
    pub(crate) date: String,
    pub(crate) sunrise: String,
    pub(crate) sunset: String,
    pub(crate) hourly_forecasts: Vec<HourlyForecast>,
    pub(crate) precipitation_probabilities: Vec<PrecipitationProbability>,
}

/// Conditions and temperature for one local forecast hour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HourlyForecast {
    pub(crate) hour: u8,
    pub(crate) temperature_celsius: i16,
    pub(crate) condition: WeatherCondition,
    pub(crate) description: String,
    pub(crate) precipitation_amount: Option<PrecipitationAmount>,
}

/// Measurable or trace precipitation accumulated during one forecast hour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrecipitationAmount {
    MeasuredTenthsOfMillimetre(u16),
    Trace,
}

impl Serialize for PrecipitationAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::MeasuredTenthsOfMillimetre(tenths) if tenths % 10 == 0 => {
                serializer.serialize_u16(tenths / 10)
            }
            Self::MeasuredTenthsOfMillimetre(tenths) => {
                serializer.serialize_f64(f64::from(*tenths) / 10.0)
            }
            Self::Trace => serializer.serialize_str("trace"),
        }
    }
}

/// Probability of precipitation during one normalized local-hour interval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrecipitationProbability {
    pub(crate) period: String,
    pub(crate) percent: u8,
}

/// Supported visual condition derived from an AEMET condition code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum WeatherCondition {
    #[serde(rename = "cloud")]
    Cloud,
    #[serde(rename = "cloud-drizzle")]
    CloudDrizzle,
    #[serde(rename = "cloud-fog")]
    CloudFog,
    #[serde(rename = "cloud-lightning")]
    CloudLightning,
    #[serde(rename = "cloud-moon")]
    CloudMoon,
    #[serde(rename = "cloud-moon-rain")]
    CloudMoonRain,
    #[serde(rename = "cloud-rain")]
    CloudRain,
    #[serde(rename = "cloud-snow")]
    CloudSnow,
    #[serde(rename = "cloud-sun")]
    CloudSun,
    #[serde(rename = "cloud-sun-rain")]
    CloudSunRain,
    #[serde(rename = "cloudy")]
    Cloudy,
    #[serde(rename = "moon")]
    Moon,
    #[serde(rename = "snowflake")]
    Snowflake,
    #[serde(rename = "sun")]
    Sun,
}

impl WeatherCondition {
    pub(super) fn from_aemet_code(code: &str) -> Option<Self> {
        match code {
            "14" => Some(Self::Cloud),
            "24" | "24n" | "25" | "26" => Some(Self::CloudDrizzle),
            "81" | "81n" | "82" | "82n" | "83" | "83n" => Some(Self::CloudFog),
            "51" | "51n" | "52" | "52n" | "53" | "53n" | "54" | "54n" | "61" | "61n" | "62"
            | "62n" | "63" | "63n" | "64" | "64n" => Some(Self::CloudLightning),
            "12n" | "13n" | "14n" | "17n" => Some(Self::CloudMoon),
            "23n" | "25n" | "26n" | "43n" | "44n" => Some(Self::CloudMoonRain),
            "44" | "45" | "45n" | "46" | "46n" => Some(Self::CloudRain),
            "33" | "33n" | "34" | "34n" | "35" | "35n" | "36" | "36n" => Some(Self::CloudSnow),
            "12" | "13" | "17" => Some(Self::CloudSun),
            "23" | "43" => Some(Self::CloudSunRain),
            "15" | "15n" | "16" | "16n" => Some(Self::Cloudy),
            "11n" => Some(Self::Moon),
            "71" | "71n" | "72" | "72n" | "73" | "73n" | "74" | "74n" => Some(Self::Snowflake),
            "11" => Some(Self::Sun),
            _ => None,
        }
    }
}
