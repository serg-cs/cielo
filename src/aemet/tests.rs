use std::{io::Write, time::Duration};

use flate2::{Compression, write::GzEncoder};
use reqwest::Url;

use super::*;

#[test]
fn parses_iso_8859_15_municipalities() {
    let mut input = br#"[{"id":"id28079","nombre":"Pe"#.to_vec();
    input.push(0xF1);
    input.extend_from_slice(br#"alara"}]"#);

    let municipalities = parse_municipalities(&input).expect("municipalities should parse");

    assert_eq!(
        municipalities.get("28079").map(String::as_str),
        Some("Peñalara")
    );
}

#[test]
fn repairs_forecast_mojibake() {
    assert_eq!(
        repair_iso_8859_15_mojibake("ValÃšncia/Valencia"),
        "València/Valencia"
    );
    assert_eq!(repair_iso_8859_15_mojibake("A Coruña"), "A Coruña");
}

#[test]
fn parses_and_orders_forecast_temperatures() {
    let archive = forecast_archive(
        "localidad_h_28079.json",
        r#"{
            "root": {
                "id": "28079",
                "elaborado": "2026-07-19T08:00:00",
                "nombre": "Madrid",
                "provincia": "Madrid",
                "prediccion": {
                    "dia": [
                        {
                            "fecha": "2026-07-20",
                            "orto": "06:55",
                            "ocaso": "21:42",
                            "estado_cielo": [{
                                "periodo": "00",
                                "valor": "24",
                                "descripcion": "  Lluvia dÃ©bil  "
                            }],
                            "temperatura": {"periodo": "00", "valor": "21"}
                        },
                        {
                            "fecha": "2026-07-19",
                            "orto": "06:54",
                            "ocaso": "21:43",
                            "estado_cielo": [
                                {"periodo": "11", "valor": "11n", "descripcion": "Despejado"},
                                {"periodo": "09", "valor": "14", "descripcion": "Nuboso"},
                                {"periodo": "10", "valor": "13", "descripcion": "  Intervalos nubosos  "}
                            ],
                            "temperatura": [
                                {"periodo": "11", "valor": "29"},
                                {"periodo": "10", "valor": "27"}
                            ]
                        }
                    ]
                }
            }
        }"#,
    );

    let forecasts = parse_forecast_archive(&archive).expect("archive should parse");

    assert_eq!(forecasts.len(), 1);
    assert_eq!(
        forecasts[0].daily_forecasts,
        vec![
            DailyForecast {
                date: "2026-07-19".to_owned(),
                sunrise: "06:54".to_owned(),
                sunset: "21:43".to_owned(),
                hourly_forecasts: vec![
                    HourlyForecast {
                        hour: 10,
                        temperature_celsius: 27,
                        condition: WeatherCondition::CloudSun,
                        description: "Intervalos nubosos".to_owned(),
                    },
                    HourlyForecast {
                        hour: 11,
                        temperature_celsius: 29,
                        condition: WeatherCondition::Moon,
                        description: "Despejado".to_owned(),
                    },
                ],
            },
            DailyForecast {
                date: "2026-07-20".to_owned(),
                sunrise: "06:55".to_owned(),
                sunset: "21:42".to_owned(),
                hourly_forecasts: vec![HourlyForecast {
                    hour: 0,
                    temperature_celsius: 21,
                    condition: WeatherCondition::CloudDrizzle,
                    description: "Lluvia débil".to_owned(),
                }],
            },
        ]
    );
}

#[test]
fn parses_and_orders_daily_temperature_extrema() {
    let archive = forecast_archive(
        "localidad_28079.json",
        r#"{
            "root": {
                "id": "28079",
                "elaborado": "2026-07-31T08:00:00",
                "prediccion": {
                    "dia": [
                        {
                            "fecha": "2026-08-01",
                            "temperatura": {"minima": 22, "maxima": 37}
                        },
                        {
                            "fecha": "2026-07-31",
                            "temperatura": {"minima": "25", "maxima": "36"}
                        }
                    ]
                }
            }
        }"#,
    );

    let forecasts = parse_daily_forecast_archive(&archive).expect("daily archive should parse");

    assert_eq!(forecasts.len(), 1);
    assert_eq!(forecasts[0].id, "28079");
    assert_eq!(
        forecasts[0].summaries,
        vec![
            DailySummary {
                date: "2026-07-31".to_owned(),
                minimum_temperature_celsius: 25,
                maximum_temperature_celsius: 36,
            },
            DailySummary {
                date: "2026-08-01".to_owned(),
                minimum_temperature_celsius: 22,
                maximum_temperature_celsius: 37,
            },
        ]
    );
}

#[test]
fn rejects_invalid_daily_temperature_ranges() {
    let archive = forecast_archive(
        "localidad_28079.json",
        r#"{
            "root": {
                "id": "28079",
                "elaborado": "2026-07-31T08:00:00",
                "prediccion": {
                    "dia": [{
                        "fecha": "2026-07-31",
                        "temperatura": {"minima": "37", "maxima": "25"}
                    }]
                }
            }
        }"#,
    );

    let error = parse_daily_forecast_archive(&archive).expect_err("invalid extrema should fail");

    assert!(error.to_string().contains("above its maximum"));
}

#[test]
fn normalizes_scalar_sky_state_shape() {
    let archive = forecast_archive(
        "localidad_h_28079.json",
        r#"{
            "root": {
                "id": "28079",
                "elaborado": "2026-07-19T08:00:00",
                "nombre": "Madrid",
                "provincia": "Madrid",
                "prediccion": {
                    "dia": [{
                        "fecha": "2026-07-19",
                        "orto": "06:54",
                        "ocaso": "21:43",
                        "estado_cielo": {
                            "periodo": "10",
                            "valor": "11",
                            "descripcion": "Despejado"
                        },
                        "temperatura": {"periodo": "10", "valor": "27"}
                    }]
                }
            }
        }"#,
    );

    let forecasts = parse_forecast_archive(&archive).expect("scalar condition should parse");

    assert_eq!(forecasts.len(), 1);
    assert_eq!(
        forecasts[0].daily_forecasts,
        vec![DailyForecast {
            date: "2026-07-19".to_owned(),
            sunrise: "06:54".to_owned(),
            sunset: "21:43".to_owned(),
            hourly_forecasts: vec![HourlyForecast {
                hour: 10,
                temperature_celsius: 27,
                condition: WeatherCondition::Sun,
                description: "Despejado".to_owned(),
            }],
        }]
    );
}

#[test]
fn rejects_invalid_solar_times() {
    for (sunrise, sunset) in [("6:54", "21:43"), ("06:60", "21:43"), ("21:43", "06:54")] {
        assert!(
            normalize_single_day_with_solar_times(
                &serde_json::json!({"periodo": "10", "valor": "27"}),
                &serde_json::json!({
                    "periodo": "10",
                    "valor": "11",
                    "descripcion": "Despejado"
                }),
                sunrise,
                sunset,
            )
            .is_err()
        );
    }
}

#[test]
fn requires_solar_times() {
    let document = serde_json::from_value::<ForecastDocument>(serde_json::json!({
        "root": {
            "id": "28079",
            "elaborado": "2026-07-19T08:00:00",
            "nombre": "Madrid",
            "provincia": "Madrid",
            "prediccion": {
                "dia": [{
                    "fecha": "2026-07-19",
                    "estado_cielo": {
                        "periodo": "10",
                        "valor": "11",
                        "descripcion": "Despejado"
                    },
                    "temperatura": {"periodo": "10", "valor": "27"}
                }]
            }
        }
    }));

    assert!(document.is_err());
}

#[test]
fn maps_every_supported_aemet_condition_code() {
    let cases: &[(WeatherCondition, &str, &[&str])] = &[
        (WeatherCondition::Cloud, "cloud", &["14"]),
        (
            WeatherCondition::CloudDrizzle,
            "cloud-drizzle",
            &["24", "24n", "25", "26"],
        ),
        (
            WeatherCondition::CloudFog,
            "cloud-fog",
            &["81", "81n", "82", "82n", "83", "83n"],
        ),
        (
            WeatherCondition::CloudLightning,
            "cloud-lightning",
            &[
                "51", "51n", "52", "52n", "53", "53n", "54", "54n", "61", "61n", "62", "62n", "63",
                "63n", "64", "64n",
            ],
        ),
        (
            WeatherCondition::CloudMoon,
            "cloud-moon",
            &["12n", "13n", "14n", "17n"],
        ),
        (
            WeatherCondition::CloudMoonRain,
            "cloud-moon-rain",
            &["23n", "25n", "26n", "43n", "44n"],
        ),
        (
            WeatherCondition::CloudRain,
            "cloud-rain",
            &["44", "45", "45n", "46", "46n"],
        ),
        (
            WeatherCondition::CloudSnow,
            "cloud-snow",
            &["33", "33n", "34", "34n", "35", "35n", "36", "36n"],
        ),
        (WeatherCondition::CloudSun, "cloud-sun", &["12", "13", "17"]),
        (
            WeatherCondition::CloudSunRain,
            "cloud-sun-rain",
            &["23", "43"],
        ),
        (
            WeatherCondition::Cloudy,
            "cloudy",
            &["15", "15n", "16", "16n"],
        ),
        (WeatherCondition::Moon, "moon", &["11n"]),
        (
            WeatherCondition::Snowflake,
            "snowflake",
            &["71", "71n", "72", "72n", "73", "73n", "74", "74n"],
        ),
        (WeatherCondition::Sun, "sun", &["11"]),
    ];

    let mut mapped_code_count = 0;
    for (expected_state, wire_value, codes) in cases {
        assert_eq!(
            serde_json::to_value(expected_state).expect("state should serialize"),
            serde_json::Value::String((*wire_value).to_owned())
        );
        for code in *codes {
            assert_eq!(
                WeatherCondition::from_aemet_code(code),
                Some(*expected_state)
            );
            mapped_code_count += 1;
        }
    }

    assert_eq!(mapped_code_count, 68);
}

#[test]
fn prolongs_latest_earlier_condition_for_missing_hour() {
    let forecast = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([
            {"periodo": "11", "valor": "11", "descripcion": "Despejado"},
            {"periodo": "09", "valor": "13", "descripcion": "Intervalos nubosos"}
        ]),
    )
    .expect("forecast should normalize")
    .expect("forecast should remain included");

    assert_eq!(
        forecast.daily_forecasts[0].hourly_forecasts[0].condition,
        WeatherCondition::CloudSun
    );
    assert_eq!(
        forecast.daily_forecasts[0].hourly_forecasts[0].description,
        "Intervalos nubosos"
    );
}

#[test]
fn uses_closest_later_condition_before_first_available_state() {
    let forecast = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([
            {"periodo": "12", "valor": "13", "descripcion": "Intervalos nubosos"},
            {"periodo": "11", "valor": "11", "descripcion": "Despejado"}
        ]),
    )
    .expect("forecast should normalize")
    .expect("forecast should remain included");

    assert_eq!(
        forecast.daily_forecasts[0].hourly_forecasts[0].condition,
        WeatherCondition::Sun
    );
    assert_eq!(
        forecast.daily_forecasts[0].hourly_forecasts[0].description,
        "Despejado"
    );
}

#[test]
fn omits_forecast_without_any_conditions() {
    let forecast = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([]),
    )
    .expect("conditionless forecast should not fail the snapshot");

    assert!(forecast.is_none());
}

#[test]
fn rejects_archive_when_every_municipality_lacks_conditions() {
    let archive = forecast_archive(
        "localidad_h_01001.json",
        r#"{
            "root": {
                "id": "01001",
                "elaborado": "2026-07-22T08:00:00",
                "nombre": "Alegría-Dulantzi",
                "provincia": "Araba/Álava",
                "prediccion": {
                    "dia": [{
                        "fecha": "2026-07-22",
                        "orto": "07:02",
                        "ocaso": "21:36",
                        "temperatura": {"periodo": "09", "valor": "18"}
                    }]
                }
            }
        }"#,
    );

    let error = parse_forecast_archive(&archive)
        .expect_err("an entirely unusable archive should not generate an empty dataset");

    assert!(
        error
            .to_string()
            .contains("does not contain any forecasts with sky conditions")
    );
}

#[test]
fn archive_excludes_conditionless_municipality_but_keeps_usable_forecasts() {
    let conditionless = r#"{
        "root": {
            "id": "01001",
            "elaborado": "2026-07-22T08:00:00",
            "nombre": "Alegría-Dulantzi",
            "provincia": "Araba/Álava",
            "prediccion": {
                "dia": [{
                    "fecha": "2026-07-22",
                    "orto": "07:02",
                    "ocaso": "21:36",
                    "temperatura": {"periodo": "09", "valor": "18"}
                }]
            }
        }
    }"#;
    let usable = r#"{
        "root": {
            "id": "01002",
            "elaborado": "2026-07-22T08:00:00",
            "nombre": "Amurrio",
            "provincia": "Araba/Álava",
            "prediccion": {
                "dia": [{
                    "fecha": "2026-07-22",
                    "orto": "07:02",
                    "ocaso": "21:36",
                    "estado_cielo": [{
                        "periodo": "09",
                        "valor": "11",
                        "descripcion": "Despejado"
                    }],
                    "temperatura": {"periodo": "09", "valor": "19"}
                }]
            }
        }
    }"#;
    let archive = forecast_archive_entries(&[
        ("localidad_h_01001.json", conditionless),
        ("localidad_h_01002.json", usable),
    ]);

    let forecasts = parse_forecast_archive(&archive).expect("usable forecast should remain");

    assert_eq!(forecasts.len(), 1);
    assert_eq!(forecasts[0].id, "01002");
}

#[test]
fn rejects_empty_condition_codes_and_descriptions() {
    for (sky_state, expected_message) in [
        (
            serde_json::json!({
                "periodo": "10",
                "valor": "  ",
                "descripcion": "Despejado"
            }),
            "empty condition code",
        ),
        (
            serde_json::json!({
                "periodo": "10",
                "valor": "11",
                "descripcion": "  "
            }),
            "empty condition description",
        ),
    ] {
        let error = normalize_single_day(
            &serde_json::json!({"periodo": "10", "valor": "27"}),
            &serde_json::json!([sky_state]),
        )
        .expect_err("empty condition values should fail");

        assert!(error.to_string().contains(expected_message));
    }
}

#[test]
fn rejects_invalid_and_duplicate_condition_hours() {
    let invalid_error = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([{
            "periodo": "24",
            "valor": "11",
            "descripcion": "Despejado"
        }]),
    )
    .expect_err("out-of-range condition hours should fail");
    assert!(invalid_error.to_string().contains("invalid condition hour"));

    let duplicate_error = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([
            {"periodo": "10", "valor": "11", "descripcion": "Despejado"},
            {"periodo": "10", "valor": "13", "descripcion": "Intervalos nubosos"}
        ]),
    )
    .expect_err("duplicate condition hours should fail");
    assert!(
        duplicate_error
            .to_string()
            .contains("duplicate condition hours")
    );
}

#[test]
fn rejects_unknown_condition_codes_even_for_extra_hours() {
    let error = normalize_single_day(
        &serde_json::json!({"periodo": "10", "valor": "27"}),
        &serde_json::json!([
            {"periodo": "10", "valor": "11", "descripcion": "Despejado"},
            {"periodo": "09", "valor": "99", "descripcion": "Estado nuevo"}
        ]),
    )
    .expect_err("unknown conditions should fail the snapshot");

    assert!(error.to_string().contains("unknown condition code"));
}

#[test]
fn rejects_forecast_filename_id_mismatch() {
    let archive = forecast_archive(
        "localidad_h_28080.json",
        r#"{
            "root": {
                "id": "28079",
                "elaborado": "2026-07-19T08:00:00",
                "nombre": "Madrid",
                "provincia": "Madrid",
                "prediccion": {
                    "dia": [{
                        "fecha": "2026-07-19",
                        "orto": "06:54",
                        "ocaso": "21:43",
                        "temperatura": {"periodo": "10", "valor": "27"}
                    }]
                }
            }
        }"#,
    );

    let error = parse_forecast_archive(&archive).expect_err("mismatch should fail");

    assert!(error.to_string().contains("does not match document ID"));
}

#[tokio::test]
async fn follows_an_authenticated_envelope_to_same_origin_data() {
    let mut server = mockito::Server::new_async().await;
    let data_url = format!("{}/data", server.url());
    let envelope = format!(r#"{{"descripcion":"ok","estado":200,"datos":"{data_url}"}}"#);
    let product_mock = server
        .mock("GET", "/product")
        .match_header("api_key", "test-key")
        .with_status(200)
        .with_body(envelope)
        .create_async()
        .await;
    let data_mock = server
        .mock("GET", "/data")
        .with_status(200)
        .with_body("weather-data")
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let result = client
        .fetch_product("product", 1024)
        .await
        .expect("product should download");

    assert_eq!(result, b"weather-data");
    product_mock.assert_async().await;
    data_mock.assert_async().await;
}

#[tokio::test]
async fn fetches_and_normalizes_all_weather_products() {
    let mut server = mockito::Server::new_async().await;
    let products = [
        (
            "/api/maestro/municipios",
            "/data/municipalities",
            br#"[{"id":"id28079","nombre":"Madrid"}]"#.to_vec(),
        ),
        (
            "/api/prediccion/especifica/municipio/horaria/todos",
            "/data/hourly",
            forecast_archive(
                "localidad_h_28079.json",
                r#"{
                    "root": {
                        "id": "28079",
                        "elaborado": "2026-07-31T08:00:00",
                        "nombre": "Madrid",
                        "provincia": "Madrid",
                        "prediccion": {
                            "dia": [{
                                "fecha": "2026-07-31",
                                "orto": "07:10",
                                "ocaso": "21:30",
                                "estado_cielo": {
                                    "periodo": "10",
                                    "valor": "11",
                                    "descripcion": "Despejado"
                                },
                                "temperatura": {"periodo": "10", "valor": "27"}
                            }]
                        }
                    }
                }"#,
            ),
        ),
        (
            "/api/prediccion/especifica/municipio/diaria/todos",
            "/data/daily",
            forecast_archive(
                "localidad_28079.json",
                r#"{
                    "root": {
                        "id": "28079",
                        "elaborado": "2026-07-31T08:00:00",
                        "prediccion": {
                            "dia": [{
                                "fecha": "2026-07-31",
                                "temperatura": {"minima": "25", "maxima": "36"}
                            }]
                        }
                    }
                }"#,
            ),
        ),
    ];
    let mut mocks = Vec::new();
    for (product_path, data_path, body) in products {
        let data_url = format!("{}{data_path}", server.url());
        let envelope = format!(r#"{{"descripcion":"ok","estado":200,"datos":"{data_url}"}}"#);
        mocks.push(
            server
                .mock("GET", product_path)
                .match_header("api_key", "test-key")
                .with_status(200)
                .with_body(envelope)
                .create_async()
                .await,
        );
        mocks.push(
            server
                .mock("GET", data_path)
                .with_status(200)
                .with_body(body)
                .create_async()
                .await,
        );
    }
    let client = test_client(&server, Duration::ZERO);

    let weather_data = client.fetch().await.expect("all products should parse");

    assert_eq!(weather_data.municipalities.len(), 1);
    assert_eq!(weather_data.forecasts.len(), 1);
    assert_eq!(weather_data.daily_forecasts.len(), 1);
    for product_mock in mocks {
        product_mock.assert_async().await;
    }
}

#[tokio::test]
async fn rejects_cross_origin_data_urls() {
    let mut server = mockito::Server::new_async().await;
    let envelope = r#"{
        "descripcion": "ok",
        "estado": 200,
        "datos": "https://example.com/untrusted"
    }"#;
    let product_mock = server
        .mock("GET", "/product")
        .with_status(200)
        .with_body(envelope)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("cross-origin URL should fail");

    assert!(error.to_string().contains("untrusted data URL"));
    product_mock.assert_async().await;
}

#[tokio::test]
async fn does_not_follow_redirects_from_authenticated_endpoints() {
    let mut source_server = mockito::Server::new_async().await;
    let mut target_server = mockito::Server::new_async().await;
    let target_mock = target_server
        .mock("GET", "/credential-target")
        .expect(0)
        .create_async()
        .await;
    let source_mock = source_server
        .mock("GET", "/product")
        .match_header("api_key", "test-key")
        .with_status(302)
        .with_header(
            "location",
            &format!("{}/credential-target", target_server.url()),
        )
        .create_async()
        .await;
    let client = test_client(&source_server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("redirect should fail");

    assert!(error.to_string().contains("HTTP status 302"));
    source_mock.assert_async().await;
    target_mock.assert_async().await;
}

#[tokio::test]
async fn does_not_follow_or_expose_redirecting_data_urls() {
    let mut source_server = mockito::Server::new_async().await;
    let mut target_server = mockito::Server::new_async().await;
    let target_mock = target_server
        .mock("GET", "/redirect-target")
        .expect(0)
        .create_async()
        .await;
    let data_url = format!("{}/opaque-access-token", source_server.url());
    let envelope = format!(r#"{{"descripcion":"ok","estado":200,"datos":"{data_url}"}}"#);
    let product_mock = source_server
        .mock("GET", "/product")
        .with_status(200)
        .with_body(envelope)
        .create_async()
        .await;
    let data_mock = source_server
        .mock("GET", "/opaque-access-token")
        .with_status(302)
        .with_header(
            "location",
            &format!("{}/redirect-target", target_server.url()),
        )
        .create_async()
        .await;
    let client = test_client(&source_server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("redirect should fail");
    let error_message = format!("{error:#}");

    assert!(error_message.contains("HTTP status 302"));
    assert!(!error_message.contains("opaque-access-token"));
    product_mock.assert_async().await;
    data_mock.assert_async().await;
    target_mock.assert_async().await;
}

#[tokio::test]
async fn rejects_data_larger_than_the_product_limit() {
    let mut server = mockito::Server::new_async().await;
    let data_url = format!("{}/data", server.url());
    let envelope = format!(r#"{{"descripcion":"ok","estado":200,"datos":"{data_url}"}}"#);
    let product_mock = server
        .mock("GET", "/product")
        .with_status(200)
        .with_body(envelope)
        .create_async()
        .await;
    let data_mock = server
        .mock("GET", "/data")
        .with_status(200)
        .with_body("too-large")
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 4)
        .await
        .expect_err("oversized data should fail");

    assert!(error.to_string().contains("too large"));
    product_mock.assert_async().await;
    data_mock.assert_async().await;
}

#[tokio::test]
async fn retries_server_errors() {
    let mut server = mockito::Server::new_async().await;
    let product_mock = server
        .mock("GET", "/product")
        .with_status(503)
        .expect(MAX_ATTEMPTS)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("persistent server error should fail");

    assert!(error.to_string().contains("HTTP status 503"));
    product_mock.assert_async().await;
}

#[tokio::test]
async fn retries_transient_statuses_inside_aemet_envelopes() {
    let mut server = mockito::Server::new_async().await;
    let product_mock = server
        .mock("GET", "/product")
        .with_status(200)
        .with_body(r#"{"descripcion":"busy","estado":503}"#)
        .expect(MAX_ATTEMPTS)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("persistent envelope error should fail");

    assert!(error.to_string().contains("status 503"));
    product_mock.assert_async().await;
}

#[test]
fn parses_retry_after_seconds_and_http_dates() {
    assert_eq!(parse_retry_after("120"), Some(Duration::from_mins(2)));
    assert_eq!(
        parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"),
        Some(Duration::ZERO)
    );
    assert_eq!(parse_retry_after("not-a-date"), None);
}

#[test]
fn applies_a_longer_backoff_to_rate_limits() {
    let delays = (0..MAX_ATTEMPTS - 1)
        .map(|attempt| {
            retry_delay(
                attempt,
                Duration::from_secs(1),
                None,
                RetryKind::RateLimited,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        delays,
        [
            Duration::from_secs(15),
            Duration::from_secs(30),
            Duration::from_mins(1),
        ]
    );
}

#[test]
fn preserves_transient_backoff_and_rate_limit_minimums() {
    assert_eq!(
        retry_delay(2, Duration::from_secs(1), None, RetryKind::Transient,),
        Duration::from_secs(4)
    );
    assert_eq!(
        retry_delay(
            1,
            Duration::from_secs(1),
            Some(Duration::from_secs(10)),
            RetryKind::RateLimited,
        ),
        Duration::from_secs(30)
    );
    assert_eq!(
        retry_delay(
            1,
            Duration::from_secs(1),
            Some(Duration::from_secs(90)),
            RetryKind::RateLimited,
        ),
        Duration::from_secs(90)
    );
}

#[test]
fn redacts_sensitive_url_components() {
    let url = Url::parse("https://example.com/opaque-access-token?secret=value#fragment")
        .expect("test URL should parse");

    assert_eq!(
        redact_url(&url, RequestKind::Api),
        "https://example.com/opaque-access-token"
    );
    assert_eq!(
        redact_url(&url, RequestKind::Data),
        "https://example.com/redacted"
    );
}

fn forecast_archive(path: &str, body: &str) -> Vec<u8> {
    forecast_archive_entries(&[(path, body)])
}

fn forecast_archive_entries(entries: &[(&str, &str)]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (path, body) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(body.len()).expect("test body should fit in u64"));
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, path, body.as_bytes())
            .expect("test archive entry should be written");
    }
    let mut encoder = archive
        .into_inner()
        .expect("test tar archive should finish");
    encoder.flush().expect("test gzip stream should flush");
    encoder.finish().expect("test gzip stream should finish")
}

fn normalize_single_day(
    temperatures: &serde_json::Value,
    sky_states: &serde_json::Value,
) -> Result<Option<MunicipalityForecast>> {
    normalize_single_day_with_solar_times(temperatures, sky_states, "06:54", "21:43")
}

fn normalize_single_day_with_solar_times(
    temperatures: &serde_json::Value,
    sky_states: &serde_json::Value,
    sunrise: &str,
    sunset: &str,
) -> Result<Option<MunicipalityForecast>> {
    let document = serde_json::from_value::<ForecastDocument>(serde_json::json!({
        "root": {
            "id": "28079",
            "elaborado": "2026-07-19T08:00:00",
            "nombre": "Madrid",
            "provincia": "Madrid",
            "prediccion": {
                "dia": [{
                    "fecha": "2026-07-19",
                    "orto": sunrise,
                    "ocaso": sunset,
                    "estado_cielo": sky_states,
                    "temperatura": temperatures
                }]
            }
        }
    }))
    .expect("test forecast should deserialize");
    normalize_forecast(document.root, "28079")
}

fn test_client(server: &mockito::Server, retry_base_delay: Duration) -> AemetClient {
    let base_url = Url::parse(&format!("{}/", server.url())).expect("mock URL should parse");
    AemetClient::with_base_url("test-key".to_owned(), base_url, retry_base_delay)
        .expect("test client should build")
}
