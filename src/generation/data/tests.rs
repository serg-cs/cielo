use std::{collections::BTreeMap, path::Path};

use jiff::{Timestamp, civil::date};
use serde_json::json;

use crate::{
    aemet::{
        Accumulation, DailyForecast, DailyRange, DayForecast, HourValues, HourlyForecast,
        MunicipalityTimeZone, PeriodValues, WarningLevel, WeatherWarning,
    },
    catalog::Municipality,
};

use super::{
    bundle_path,
    coast::{coastal_municipalities, distance_to_polygon_km},
    documents::{MetaDocument, MunicipalityDocument},
    report::{DataBuildReport, ProductStatus, group_failures},
};

fn utc(value: &str) -> Timestamp {
    value.parse().expect("test time should parse")
}

#[test]
fn groups_municipalities_into_twenty_id_bundles() {
    assert_eq!(
        bundle_path("28079").expect("path"),
        Path::new("v4/forecast/28/060.json")
    );
    assert_eq!(
        bundle_path("35001").expect("path"),
        Path::new("v4/forecast/35/000.json")
    );
    assert_eq!(
        bundle_path("08019").expect("path"),
        Path::new("v4/forecast/08/000.json")
    );
    assert!(bundle_path("2807").is_err());
}

#[test]
#[allow(clippy::too_many_lines)]
fn writes_columnar_hours_windows_and_daily_periods() {
    let zone = MunicipalityTimeZone::EuropeMadrid.rules().expect("zone");
    let mut hours = BTreeMap::new();
    hours.insert(
        utc("2026-10-31T06:00:00Z"),
        HourValues {
            sky: Some("11n".to_owned()),
            temperature: Some(9),
            precipitation: Some(Accumulation::Trace),
            snow: Some(Accumulation::TenthsOfMillimetre(12)),
            wind_direction: Some("NE".to_owned()),
            wind_speed: Some(11),
            ..HourValues::default()
        },
    );
    // A missing hour in between becomes nulls in every column.
    hours.insert(
        utc("2026-10-31T08:00:00Z"),
        HourValues {
            temperature: Some(12),
            precipitation: Some(Accumulation::TenthsOfMillimetre(20)),
            ..HourValues::default()
        },
    );
    let start = "2026-10-31T07:00:00[Europe/Madrid]"
        .parse::<jiff::Zoned>()
        .expect("time");
    let end = "2026-10-31T13:00:00[Europe/Madrid]"
        .parse::<jiff::Zoned>()
        .expect("time");
    let hourly = HourlyForecast {
        id: "28079".to_owned(),
        issued: Some(utc("2026-10-31T08:49:07Z")),
        hours,
        windows: vec![crate::aemet::ProbabilityWindow {
            start,
            end,
            precipitation: Some(20),
            storm: Some(5),
            snow: None,
        }],
        sun: vec![crate::aemet::SunTimes {
            date: date(2026, 10, 31),
            sunrise: Some("07:42".to_owned()),
            sunset: Some("18:13".to_owned()),
        }],
    };
    let mut periods = BTreeMap::new();
    periods.insert(
        "00-24".to_owned(),
        PeriodValues {
            sky: Some("15".to_owned()),
            precipitation_probability: Some(45),
            snow_level_metres: Some(1700),
            ..PeriodValues::default()
        },
    );
    let daily = DailyForecast {
        id: "28079".to_owned(),
        issued: Some(utc("2026-10-31T09:00:00Z")),
        days: vec![DayForecast {
            date: date(2026, 10, 31),
            temperature: DailyRange {
                minimum: Some(9),
                maximum: Some(20),
                at_hours: BTreeMap::from([(6, 10), (12, 17)]),
            },
            feels_like: DailyRange::default(),
            humidity: DailyRange {
                minimum: Some(50),
                maximum: Some(80),
                at_hours: BTreeMap::new(),
            },
            uv_index: Some(3),
            periods,
        }],
    };
    let fire = BTreeMap::from([(date(2026, 10, 31), 2)]);
    let municipality = Municipality {
        id: "28079".to_owned(),
        name: "Madrid".to_owned(),
        latitude: Some(40.416_775),
        longitude: Some(-3.703_79),
        altitude_metres: Some(657),
        population: Some(3_165_235),
        warning_zone: Some("722802".to_owned()),
    };

    let document = MunicipalityDocument::new(
        Some(&hourly),
        Some(&daily),
        MetaDocument::new(&municipality),
        Some(&fire),
        &zone,
    );

    assert_eq!(
        serde_json::to_value(&document).expect("document serializes"),
        json!({
            "issued": "2026-10-31T10:00:00+01:00",
            "meta": {"zone": "722802", "alt": 657},
            "hourly": {
                "t0": "2026-10-31T07:00:00+01:00",
                "sky": ["11n", null, null],
                "temp": [9, null, 12],
                "feels": [null, null, null],
                "rain": ["Ip", null, 2],
                "snow": [1.2, null, null],
                "rh": [null, null, null],
                "wdir": ["NE", null, null],
                "wind": [11, null, null],
                "gust": [null, null, null]
            },
            "windows": [{"t": "2026-10-31T07:00:00+01:00", "h": 6, "pop": 20, "storm": 5}],
            "sun": {"2026-10-31": ["07:42", "18:13"]},
            "daily": [{
                "date": "2026-10-31",
                "tmin": 9,
                "tmax": 20,
                "rh": [50, 80],
                "uv": 3,
                "temps": {"6": 10, "12": 17},
                "fire": 2,
                "p": {"00-24": {"sky": "15", "pop": 45, "snowl": 1700}}
            }]
        })
    );
}

#[test]
fn hourly_columns_span_the_clock_change_in_absolute_hours() {
    let zone = MunicipalityTimeZone::EuropeMadrid.rules().expect("zone");
    let hours = (0..4)
        .map(|hour| {
            (
                utc("2026-10-24T23:00:00Z")
                    .checked_add(jiff::SignedDuration::from_hours(hour))
                    .expect("time"),
                HourValues {
                    temperature: Some(10),
                    ..HourValues::default()
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let hourly = HourlyForecast {
        id: "28079".to_owned(),
        issued: None,
        hours,
        windows: vec![],
        sun: vec![],
    };
    let municipality = Municipality {
        id: "28079".to_owned(),
        name: "Madrid".to_owned(),
        latitude: None,
        longitude: None,
        altitude_metres: None,
        population: None,
        warning_zone: None,
    };
    let document = MunicipalityDocument::new(
        Some(&hourly),
        None,
        MetaDocument::new(&municipality),
        None,
        &zone,
    );
    let value = serde_json::to_value(document).expect("serializes");
    assert_eq!(value["hourly"]["t0"], "2026-10-25T01:00:00+02:00");
    assert_eq!(value["hourly"]["temp"].as_array().expect("temps").len(), 4);
}

#[test]
fn coastal_warnings_reach_nearby_municipalities_of_the_province() {
    let square = vec![(41.0, 2.0), (41.0, 2.5), (41.5, 2.5), (41.5, 2.0)];
    assert_eq!(distance_to_polygon_km((41.2, 2.2), &square), 0.0);
    let east = distance_to_polygon_km((41.2, 1.9), &square);
    assert!(
        (east - 8.4).abs() < 0.3,
        "0.1 degrees of longitude at 41N is about 8.4 km, got {east}"
    );

    let warning = WeatherWarning {
        id: "w".to_owned(),
        level: WarningLevel::Yellow,
        code: Some("CO".to_owned()),
        name: "Costeros".to_owned(),
        onset: None,
        expires: None,
        description: None,
        parameter: None,
        probability: None,
        zones: vec!["690801C".to_owned()],
        area: None,
        polygons: vec![square],
    };
    let municipality = |id: &str, latitude: f64, longitude: f64| Municipality {
        id: id.to_owned(),
        name: String::new(),
        latitude: Some(latitude),
        longitude: Some(longitude),
        altitude_metres: None,
        population: None,
        warning_zone: None,
    };
    let municipalities = [
        municipality("08019", 41.38, 1.98),
        municipality("08001", 41.38, 1.5),
        municipality("17079", 41.38, 1.98),
    ];
    assert_eq!(
        coastal_municipalities(&warning, municipalities.iter()),
        ["08019"]
    );
}

#[test]
fn reports_product_status_and_grouped_failures() {
    let failures = ["28079", "28080"]
        .into_iter()
        .map(|id| crate::aemet::EntryFailure {
            id: id.to_owned(),
            product: crate::aemet::AemetProduct::Hourly,
            error: anyhow::anyhow!("municipality {id} has 3 problems"),
        })
        .collect::<Vec<_>>();
    let report = DataBuildReport {
        products: vec![
            (
                crate::aemet::AemetProduct::Hourly,
                ProductStatus::Published(8000),
            ),
            (
                crate::aemet::AemetProduct::Warnings,
                ProductStatus::Failed("timeout | retry".to_owned()),
            ),
        ],
        source_entries: 8002,
        published: 8000,
        failed: 2,
        groups: group_failures(&failures),
    };
    let text = report.render_text();
    assert!(text.contains("2 x municipality # has # problems"));
    assert!(text.contains("warnings: failed: timeout"));
    assert!(report.render_markdown().contains("timeout \\| retry"));
}
