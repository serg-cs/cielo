//! Published JSON documents (schema v4).

use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp, Zoned, tz::TimeZone};
use serde::Serialize;

use crate::{
    aemet::{
        Accumulation, BeachForecast, DailyForecast, DailyRange, FireDanger, HourValues,
        HourlyForecast, PeriodValues, WarningLevel, WeatherWarning,
    },
    catalog::Municipality,
};

pub(crate) const SCHEMA_VERSION: u8 = 4;

/// RFC 3339 with the local UTC offset, such as `2026-10-25T02:00:00+02:00`.
pub(super) fn local_time(timestamp: Timestamp, time_zone: &TimeZone) -> String {
    let timestamp = timestamp.round(jiff::Unit::Second).unwrap_or(timestamp);
    let offset = time_zone.to_offset(timestamp);
    timestamp.display_with_offset(offset).to_string()
}

fn zoned_time(time: &Zoned) -> String {
    time.timestamp()
        .display_with_offset(time.offset())
        .to_string()
}

#[derive(Debug, Serialize)]
pub(super) struct ProvinceDocument {
    pub(super) code: String,
    pub(super) name: String,
}

/// `v4/places.json`: the searchable municipality list, as parallel columns.
#[derive(Debug, Default, Serialize)]
pub(super) struct PlacesDocument {
    pub(super) schema: u8,
    pub(super) issued: String,
    pub(super) provinces: Vec<ProvinceDocument>,
    pub(super) ids: Vec<String>,
    pub(super) names: Vec<String>,
    /// Population, used to rank search results.
    pub(super) pop: Vec<Option<u32>>,
    pub(super) beaches: BeachPlacesDocument,
}

/// Beaches with a forecast, as parallel columns.
#[derive(Debug, Default, Serialize)]
pub(super) struct BeachPlacesDocument {
    pub(super) ids: Vec<String>,
    pub(super) names: Vec<String>,
    /// Municipality of each beach.
    pub(super) m: Vec<String>,
}

/// What the app needs to know about a municipality besides its forecast.
#[derive(Debug, Serialize)]
pub(super) struct MetaDocument<'a> {
    /// Meteoalerta warning zone, to match warnings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) zone: Option<&'a str>,
    /// Altitude in metres, shown next to the snow level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) alt: Option<i32>,
}

impl<'a> MetaDocument<'a> {
    pub(super) fn new(municipality: &'a Municipality) -> Self {
        Self {
            zone: municipality.warning_zone.as_deref(),
            alt: municipality.altitude_metres,
        }
    }
}

/// Hourly values as parallel columns starting at `t0`, one hour apart in
/// absolute time; hours without data are `null`.
#[derive(Debug, Serialize)]
pub(super) struct HourlyColumns<'a> {
    pub(super) t0: String,
    pub(super) sky: Vec<Option<&'a str>>,
    pub(super) temp: Vec<Option<i16>>,
    pub(super) feels: Vec<Option<i16>>,
    pub(super) rain: Vec<Option<Accumulation>>,
    pub(super) snow: Vec<Option<Accumulation>>,
    pub(super) rh: Vec<Option<u8>>,
    pub(super) wdir: Vec<Option<&'a str>>,
    pub(super) wind: Vec<Option<u16>>,
    pub(super) gust: Vec<Option<u16>>,
}

impl<'a> HourlyColumns<'a> {
    pub(super) fn new(
        hours: &'a BTreeMap<Timestamp, HourValues>,
        time_zone: &TimeZone,
    ) -> Option<Self> {
        let (first, _) = hours.first_key_value()?;
        let (last, _) = hours.last_key_value()?;
        let span = last.duration_since(*first).as_hours();
        let count = usize::try_from(span).ok()? + 1;
        let mut columns = Self {
            t0: local_time(*first, time_zone),
            sky: Vec::with_capacity(count),
            temp: Vec::with_capacity(count),
            feels: Vec::with_capacity(count),
            rain: Vec::with_capacity(count),
            snow: Vec::with_capacity(count),
            rh: Vec::with_capacity(count),
            wdir: Vec::with_capacity(count),
            wind: Vec::with_capacity(count),
            gust: Vec::with_capacity(count),
        };
        for index in 0..count {
            let hours_after = i64::try_from(index).ok()?;
            let time = first
                .checked_add(SignedDuration::from_hours(hours_after))
                .ok()?;
            let values = hours.get(&time);
            columns
                .sky
                .push(values.and_then(|values| values.sky.as_deref()));
            columns
                .temp
                .push(values.and_then(|values| values.temperature));
            columns
                .feels
                .push(values.and_then(|values| values.feels_like));
            columns
                .rain
                .push(values.and_then(|values| values.precipitation));
            columns.snow.push(values.and_then(|values| values.snow));
            columns.rh.push(values.and_then(|values| values.humidity));
            columns
                .wdir
                .push(values.and_then(|values| values.wind_direction.as_deref()));
            columns
                .wind
                .push(values.and_then(|values| values.wind_speed));
            columns.gust.push(values.and_then(|values| values.gust));
        }
        Some(columns)
    }
}

#[derive(Debug, Serialize)]
pub(super) struct WindowDocument {
    /// Start of the window.
    pub(super) t: String,
    /// Length in hours (5 or 7 on days the clocks change).
    pub(super) h: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) pop: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) storm: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) snow: Option<u8>,
}

#[derive(Debug, Serialize)]
pub(super) struct PeriodDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sky: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) pop: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) snowl: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) wdir: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) wind: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) gust: Option<u16>,
}

impl<'a> From<&'a PeriodValues> for PeriodDocument<'a> {
    fn from(values: &'a PeriodValues) -> Self {
        Self {
            sky: values.sky.as_deref(),
            pop: values.precipitation_probability,
            snowl: values.snow_level_metres,
            wdir: values.wind_direction.as_deref(),
            wind: values.wind_speed,
            gust: values.gust,
        }
    }
}

fn range_pair(range: &DailyRange) -> Option<[Option<i16>; 2]> {
    (range.minimum.is_some() || range.maximum.is_some()).then_some([range.minimum, range.maximum])
}

#[derive(Debug, Serialize)]
pub(super) struct DayDocument<'a> {
    pub(super) date: String,
    pub(super) tmin: Option<i16>,
    pub(super) tmax: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) feels: Option<[Option<i16>; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) rh: Option<[Option<i16>; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) uv: Option<u8>,
    /// Temperatures at local hours 6, 12, 18 and 24.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) temps: BTreeMap<u8, i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fire: Option<u8>,
    /// Values by period: `00-24`, `00-12`, `12-24`, `00-06` ... `18-24`.
    pub(super) p: BTreeMap<&'a str, PeriodDocument<'a>>,
}

/// One municipality inside a forecast bundle.
#[derive(Debug, Serialize)]
pub(super) struct MunicipalityDocument<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) issued: Option<String>,
    pub(super) meta: MetaDocument<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) hourly: Option<HourlyColumns<'a>>,
    pub(super) windows: Vec<WindowDocument>,
    /// Local sunrise and sunset by date.
    pub(super) sun: BTreeMap<String, [Option<&'a str>; 2]>,
    pub(super) daily: Vec<DayDocument<'a>>,
}

impl<'a> MunicipalityDocument<'a> {
    pub(super) fn new(
        hourly: Option<&'a HourlyForecast>,
        daily: Option<&'a DailyForecast>,
        meta: MetaDocument<'a>,
        fire: Option<&FireDanger>,
        time_zone: &TimeZone,
    ) -> Self {
        let issued = hourly
            .and_then(|forecast| forecast.issued)
            .max(daily.and_then(|forecast| forecast.issued))
            .map(|issued| local_time(issued, time_zone));
        let windows = hourly
            .map(|forecast| {
                forecast
                    .windows
                    .iter()
                    .map(|window| WindowDocument {
                        t: zoned_time(&window.start),
                        h: window
                            .end
                            .timestamp()
                            .duration_since(window.start.timestamp())
                            .as_hours(),
                        pop: window.precipitation,
                        storm: window.storm,
                        snow: window.snow,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sun = hourly
            .map(|forecast| {
                forecast
                    .sun
                    .iter()
                    .map(|sun| {
                        (
                            sun.date.to_string(),
                            [sun.sunrise.as_deref(), sun.sunset.as_deref()],
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let days = daily
            .map(|forecast| {
                forecast
                    .days
                    .iter()
                    .map(|day| DayDocument {
                        date: day.date.to_string(),
                        tmin: day.temperature.minimum,
                        tmax: day.temperature.maximum,
                        feels: range_pair(&day.feels_like),
                        rh: range_pair(&day.humidity),
                        uv: day.uv_index,
                        temps: day.temperature.at_hours.clone(),
                        fire: fire.and_then(|fire| fire.get(&day.date).copied()),
                        p: day
                            .periods
                            .iter()
                            .map(|(period, values)| (period.as_str(), PeriodDocument::from(values)))
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            issued,
            meta,
            hourly: hourly.and_then(|forecast| HourlyColumns::new(&forecast.hours, time_zone)),
            windows,
            sun,
            daily: days,
        }
    }
}

/// `v4/forecast/<PP>/<NNN>.json`.
#[derive(Debug, Serialize)]
pub(super) struct BundleDocument<'a> {
    pub(super) schema: u8,
    pub(super) municipalities: BTreeMap<&'a str, MunicipalityDocument<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct WarningDocument<'a> {
    pub(super) id: &'a str,
    pub(super) level: WarningLevel,
    /// Meteoalerta phenomenon code, such as `PR`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) code: Option<&'a str>,
    /// Phenomenon in Spanish, such as `Lluvias`.
    pub(super) name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) onset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) desc: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) param: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prob: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) area: Option<&'a str>,
    pub(super) zones: &'a [String],
    /// Municipalities a coastal warning applies to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) m: Vec<&'a str>,
}

impl<'a> WarningDocument<'a> {
    pub(super) fn new(warning: &'a WeatherWarning, municipalities: Vec<&'a str>) -> Self {
        let madrid = crate::aemet::MunicipalityTimeZone::EuropeMadrid
            .rules()
            .ok();
        let time = |timestamp: Option<Timestamp>| {
            let timestamp = timestamp?;
            Some(match &madrid {
                Some(zone) => local_time(timestamp, zone),
                None => timestamp.to_string(),
            })
        };
        Self {
            id: &warning.id,
            level: warning.level,
            code: warning.code.as_deref(),
            name: &warning.name,
            onset: time(warning.onset),
            expires: time(warning.expires),
            desc: warning.description.as_deref(),
            param: warning.parameter.as_deref(),
            prob: warning.probability.as_deref(),
            area: warning.area.as_deref(),
            zones: &warning.zones,
            m: municipalities,
        }
    }
}

/// `v4/warnings.json`.
#[derive(Debug, Serialize)]
pub(super) struct WarningsDocument<'a> {
    pub(super) schema: u8,
    pub(super) warnings: Vec<WarningDocument<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct BeachDayDocument {
    pub(super) date: String,
    pub(super) sky: [Option<u16>; 2],
    pub(super) wind: [Option<u16>; 2],
    pub(super) waves: [Option<u16>; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tmax: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) water: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) feel: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) uv: Option<u8>,
}

/// One beach's forecast; its name and municipality are in `places.json`.
#[derive(Debug, Serialize)]
pub(super) struct BeachDocument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) issued: Option<String>,
    pub(super) days: Vec<BeachDayDocument>,
}

impl BeachDocument {
    pub(super) fn new(beach: &BeachForecast, time_zone: &TimeZone) -> Self {
        Self {
            issued: beach.issued.map(|issued| local_time(issued, time_zone)),
            days: beach
                .days
                .iter()
                .map(|day| BeachDayDocument {
                    date: day.date.to_string(),
                    sky: day.sky,
                    wind: day.wind,
                    waves: day.waves,
                    tmax: day.maximum_temperature,
                    water: day.water_temperature,
                    feel: day.feels_like,
                    uv: day.uv_index,
                })
                .collect(),
        }
    }
}

/// `v4/beaches/<PP>.json`: the beaches of a province, by beach code.
#[derive(Debug, Serialize)]
pub(super) struct BeachesDocument<'a> {
    pub(super) schema: u8,
    pub(super) issued: String,
    pub(super) beaches: BTreeMap<&'a str, BeachDocument>,
}
