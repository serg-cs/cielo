use std::collections::HashMap;

use serde::Serialize;

/// Complete source data needed to generate one weather-data output.
#[derive(Debug)]
pub(crate) struct AemetWeatherData {
    pub(crate) municipalities: HashMap<String, String>,
    pub(crate) forecasts: Vec<MunicipalityForecast>,
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

/// Solar times and hourly conditions for one local forecast day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DailyForecast {
    pub(crate) date: String,
    pub(crate) sunrise: String,
    pub(crate) sunset: String,
    pub(crate) hourly_forecasts: Vec<HourlyForecast>,
}

/// Conditions and temperature for one local forecast hour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HourlyForecast {
    pub(crate) hour: u8,
    pub(crate) temperature_celsius: i16,
    pub(crate) condition: WeatherCondition,
    pub(crate) description: String,
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
