//! Municipality forecasts: hourly and daily products.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result, bail};
use jiff::{
    Timestamp, Zoned,
    civil::{Date, DateTime},
    tz::{AmbiguousOffset, TimeZone},
};
use serde_json::Value;
use tracing::debug;

use crate::catalog::validate_municipality_id;

use super::{
    decoding::decode_iso_8859_15,
    models::{
        Accumulation, DailyForecast, DailyRange, DayForecast, HourValues, HourlyForecast,
        MunicipalityTimeZone, PeriodValues, ProbabilityWindow, SunTimes,
    },
    tree::{field_integer, field_text, integer, list, number, text},
};

/// Municipality ID from a document value such as `id28079` or `28079`.
fn municipality_id(value: &str) -> Result<String> {
    let id = value.trim().trim_start_matches("id");
    validate_municipality_id(id)?;
    Ok(id.to_owned())
}

/// Decode an AEMET document that may be UTF-8 or ISO-8859-15.
pub(super) fn decode_document(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => decode_iso_8859_15(bytes),
    }
}

/// AEMET's issue time, a Madrid local time without offset.
pub(super) fn parse_issued(value: Option<String>) -> Option<Timestamp> {
    let value = value?;
    let local = value.get(..19).unwrap_or(&value);
    let datetime = local.parse::<DateTime>().ok()?;
    let madrid = MunicipalityTimeZone::EuropeMadrid.rules().ok()?;
    madrid
        .to_ambiguous_zoned(datetime)
        .compatible()
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// `YYYY-MM-DD`, ignoring a time suffix; also accepts `YYYYMMDD`.
pub(super) fn parse_date(value: &str) -> Option<Date> {
    let value = value.trim();
    if value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..])
            .parse()
            .ok();
    }
    value.get(..10)?.parse().ok()
}

/// The document root: `{"root": {...}}` in JSON, the root element in XML.
fn document_root(value: &Value) -> &Value {
    value.get("root").unwrap_or(value)
}

/// Document ID, checked against the archive file name when there is one.
fn document_id(root: &Value, expected: Option<&str>) -> Result<String> {
    let id = field_text(root, "id")
        .map(|id| municipality_id(&id))
        .transpose()?
        .or_else(|| expected.map(str::to_owned))
        .context("forecast does not contain a municipality ID")?;
    if let Some(expected) = expected
        && expected != id
    {
        bail!("file name ID {expected} does not match document ID {id}");
    }
    Ok(id)
}

fn days(root: &Value) -> Vec<&Value> {
    root.get("prediccion")
        .map(|prediction| list(prediction, "dia"))
        .unwrap_or_default()
}

/// AEMET sky code such as `13` or `13n`.
fn sky_code(value: &Value) -> Option<String> {
    let code = text(value)?.to_ascii_lowercase();
    let (digits, suffix) = code.split_at(code.len().min(2));
    let valid = digits.len() == 2
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(suffix, "" | "n");
    valid.then_some(code)
}

fn wind_direction(value: &Value) -> Option<String> {
    let direction = text(value)?.to_ascii_uppercase();
    (direction.len() <= 3 && direction.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then_some(direction)
}

fn accumulation(value: &Value) -> Option<Accumulation> {
    let raw = text(value)?;
    if raw.eq_ignore_ascii_case("ip") {
        return Some(Accumulation::Trace);
    }
    let millimetres = number(value)?;
    if !(0.0..=1000.0).contains(&millimetres) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let tenths = (millimetres * 10.0).round() as u32;
    Some(Accumulation::TenthsOfMillimetre(tenths))
}

fn small<T: TryFrom<i64>>(value: Option<i64>) -> Option<T> {
    value.and_then(|value| T::try_from(value).ok())
}

/// Parse one hourly forecast document.
pub(super) fn parse_hourly(document: &Value, expected_id: Option<&str>) -> Result<HourlyForecast> {
    let root = document_root(document);
    let id = document_id(root, expected_id)?;
    let time_zone = MunicipalityTimeZone::for_municipality(&id).rules()?;
    let mut hours = BTreeMap::<Timestamp, HourValues>::new();
    let mut windows = BTreeMap::<Timestamp, ProbabilityWindow>::new();
    let mut sun = Vec::new();

    for day in days(root) {
        let Some(date) = field_text(day, "fecha").and_then(|date| parse_date(&date)) else {
            continue;
        };
        let context = DayContext {
            date,
            time_zone: &time_zone,
        };
        let sunrise = field_text(day, "orto").filter(|time| is_clock_time(time));
        let sunset = field_text(day, "ocaso").filter(|time| is_clock_time(time));
        if sunrise.is_some() || sunset.is_some() {
            sun.push(SunTimes {
                date,
                sunrise,
                sunset,
            });
        }

        context.each_hour(day, "estado_cielo", &mut hours, |hour, item| {
            hour.sky = sky_code(item);
        });
        context.each_hour(day, "temperatura", &mut hours, |hour, item| {
            hour.temperature = small(integer(item, -70, 70));
        });
        context.each_hour(day, "sens_termica", &mut hours, |hour, item| {
            hour.feels_like = small(integer(item, -90, 90));
        });
        context.each_hour(day, "precipitacion", &mut hours, |hour, item| {
            hour.precipitation = accumulation(item);
        });
        context.each_hour(day, "nieve", &mut hours, |hour, item| {
            hour.snow = accumulation(item);
        });
        context.each_hour(day, "humedad_relativa", &mut hours, |hour, item| {
            hour.humidity = small(integer(item, 0, 100));
        });
        context.each_hour(day, "viento", &mut hours, |hour, item| {
            hour.wind_direction = item.get("direccion").and_then(wind_direction);
            hour.wind_speed = small(field_integer(item, "velocidad", 0, 500));
        });
        context.each_hour(day, "racha_max", &mut hours, |hour, item| {
            hour.gust = small(integer(item, 0, 500));
        });

        for (key, set) in [
            ("prob_precipitacion", WindowField::Precipitation),
            ("prob_tormenta", WindowField::Storm),
            ("prob_nieve", WindowField::Snow),
        ] {
            for item in list(day, key) {
                let Some(period) = field_text(item, "periodo") else {
                    continue;
                };
                let Some((start, end)) = context.window(&period) else {
                    continue;
                };
                let window =
                    windows
                        .entry(start.timestamp())
                        .or_insert_with(|| ProbabilityWindow {
                            start,
                            end,
                            precipitation: None,
                            storm: None,
                            snow: None,
                        });
                let percent = small(integer(item, 0, 100));
                match set {
                    WindowField::Precipitation => window.precipitation = percent,
                    WindowField::Storm => window.storm = percent,
                    WindowField::Snow => window.snow = percent,
                }
            }
        }
    }

    hours.retain(|_, values| !values.is_empty());
    windows.retain(|_, window| {
        window.precipitation.is_some() || window.storm.is_some() || window.snow.is_some()
    });
    if hours.is_empty() {
        bail!("hourly forecast {id} does not contain any hourly values");
    }

    Ok(HourlyForecast {
        issued: parse_issued(field_text(root, "elaborado")),
        id,
        hours,
        windows: windows.into_values().collect(),
        sun,
    })
}

#[derive(Clone, Copy)]
enum WindowField {
    Precipitation,
    Storm,
    Snow,
}

fn is_clock_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && value[..2].parse::<u8>().is_ok_and(|hour| hour < 24)
        && value[3..].parse::<u8>().is_ok_and(|minute| minute < 60)
}

struct DayContext<'a> {
    date: Date,
    time_zone: &'a TimeZone,
}

impl DayContext<'_> {
    /// Apply `apply` to the hour of each item under `key`, by its `periodo`.
    fn each_hour(
        &self,
        day: &Value,
        key: &str,
        hours: &mut BTreeMap<Timestamp, HourValues>,
        apply: impl Fn(&mut HourValues, &Value),
    ) {
        let items = list(day, key);
        let labels = items
            .iter()
            .map(|item| {
                field_text(item, "periodo")
                    .and_then(|period| period.parse::<u8>().ok())
                    .filter(|hour| *hour < 24)
            })
            .collect::<Vec<_>>();
        for (item, time) in items
            .iter()
            .zip(resolve_hours(self.date, self.time_zone, &labels))
        {
            if let Some(time) = time {
                apply(hours.entry(time).or_default(), item);
            }
        }
    }

    /// Exact start and end of a window such as `0713` or `19-01`.
    fn window(&self, period: &str) -> Option<(Zoned, Zoned)> {
        let digits = period
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(char::from)
            .collect::<String>();
        if digits.len() != 4 {
            return None;
        }
        let start_hour = digits[..2].parse::<i8>().ok().filter(|hour| *hour < 24)?;
        let end_hour = digits[2..].parse::<i8>().ok().filter(|hour| *hour <= 24)?;
        let (end_date, end_hour) = if end_hour == 24 || end_hour <= start_hour {
            (self.date.tomorrow().ok()?, end_hour % 24)
        } else {
            (self.date, end_hour)
        };
        // A window edge is a label, not a data point: use the usual reading
        // of a repeated or skipped local time.
        let start = self
            .time_zone
            .to_ambiguous_zoned(self.date.at(start_hour, 0, 0, 0))
            .compatible()
            .ok()?;
        let end = self
            .time_zone
            .to_ambiguous_zoned(end_date.at(end_hour, 0, 0, 0))
            .compatible()
            .ok()?;
        Some((start, end))
    }
}

/// Turn local hour labels, in AEMET's order, into exact moments.
///
/// A label that happens twice (clocks going back) is the earlier moment the
/// first time it appears and the later moment the second time. Labels that do
/// not exist that day, or appear more often than they happen, are dropped.
pub(super) fn resolve_hours(
    date: Date,
    time_zone: &TimeZone,
    hours: &[Option<u8>],
) -> Vec<Option<Timestamp>> {
    let mut appearances = HashMap::<u8, usize>::new();
    hours
        .iter()
        .map(|hour| {
            let hour = (*hour)?;
            let appearance = appearances.entry(hour).or_default();
            *appearance += 1;
            let ambiguous =
                time_zone.to_ambiguous_zoned(date.at(i8::try_from(hour).ok()?, 0, 0, 0));
            let resolved = match (ambiguous.offset(), *appearance) {
                (AmbiguousOffset::Unambiguous { .. }, 1) => ambiguous.unambiguous(),
                (AmbiguousOffset::Fold { .. }, 1) => ambiguous.earlier(),
                (AmbiguousOffset::Fold { .. }, 2) => ambiguous.later(),
                _ => {
                    debug!(%date, hour, "dropping hour that does not exist or repeats");
                    return None;
                }
            };
            resolved.ok().map(|zoned| zoned.timestamp())
        })
        .collect()
}

/// Normalize a daily period label such as `00-24`; a missing one is the whole day.
fn daily_period(item: &Value) -> Option<String> {
    let Some(period) = field_text(item, "periodo") else {
        return Some("00-24".to_owned());
    };
    let digits = period
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(char::from)
        .collect::<String>();
    (digits.len() == 4).then(|| format!("{}-{}", &digits[..2], &digits[2..]))
}

fn daily_range(day: &Value, key: &str, min: i64, max: i64) -> DailyRange {
    let Some(value) = day.get(key) else {
        return DailyRange::default();
    };
    let mut at_hours = BTreeMap::new();
    for item in list(value, "dato") {
        let hour = field_integer(item, "hora", 0, 24).and_then(|hour| u8::try_from(hour).ok());
        let reading = small(integer(item, min, max));
        if let (Some(hour), Some(reading)) = (hour, reading) {
            at_hours.insert(hour, reading);
        }
    }
    DailyRange {
        minimum: small(field_integer(value, "minima", min, max)),
        maximum: small(field_integer(value, "maxima", min, max)),
        at_hours,
    }
}

/// Parse one daily forecast document.
pub(super) fn parse_daily(document: &Value, expected_id: Option<&str>) -> Result<DailyForecast> {
    let root = document_root(document);
    let id = document_id(root, expected_id)?;
    let mut parsed_days = Vec::new();

    for day in days(root) {
        let Some(date) = field_text(day, "fecha").and_then(|date| parse_date(&date)) else {
            continue;
        };
        let mut periods = BTreeMap::<String, PeriodValues>::new();
        let mut each_period = |key: &str, apply: &dyn Fn(&mut PeriodValues, &Value)| {
            for item in list(day, key) {
                if let Some(period) = daily_period(item) {
                    apply(periods.entry(period).or_default(), item);
                }
            }
        };
        each_period("estado_cielo", &|period, item| period.sky = sky_code(item));
        each_period("prob_precipitacion", &|period, item| {
            period.precipitation_probability = small(integer(item, 0, 100));
        });
        each_period("cota_nieve_prov", &|period, item| {
            period.snow_level_metres = small(integer(item, 0, 6000));
        });
        each_period("viento", &|period, item| {
            period.wind_direction = item.get("direccion").and_then(wind_direction);
            period.wind_speed = small(field_integer(item, "velocidad", 0, 500));
        });
        each_period("racha_max", &|period, item| {
            period.gust = small(integer(item, 0, 500));
        });
        periods.retain(|_, values| !values.is_empty());

        parsed_days.push(DayForecast {
            date,
            temperature: daily_range(day, "temperatura", -70, 70),
            feels_like: daily_range(day, "sens_termica", -90, 90),
            humidity: daily_range(day, "humedad_relativa", 0, 100),
            uv_index: day
                .get("uv_max")
                .and_then(|value| small(integer(value, 0, 20))),
            periods,
        });
    }

    parsed_days.sort_by_key(|day| day.date);
    parsed_days.dedup_by_key(|day| day.date);
    if parsed_days.is_empty() {
        bail!("daily forecast {id} does not contain any days");
    }
    Ok(DailyForecast {
        issued: parse_issued(field_text(root, "elaborado")),
        id,
        days: parsed_days,
    })
}

/// Municipality ID from an archive file name such as `localidad_h_28079.json`.
pub(super) fn id_from_file_name(file_name: &str, prefix: &str) -> Result<String> {
    let id = file_name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .with_context(|| format!("unexpected archive entry name: {file_name}"))?;
    validate_municipality_id(id)?;
    Ok(id.to_owned())
}
