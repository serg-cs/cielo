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
                            "temperatura": {"periodo": "00", "valor": "21"}
                        },
                        {
                            "fecha": "2026-07-19",
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
        forecasts[0].temperatures,
        vec![
            Temperature {
                date: "2026-07-19".to_owned(),
                hour: 10,
                celsius: 27,
            },
            Temperature {
                date: "2026-07-19".to_owned(),
                hour: 11,
                celsius: 29,
            },
            Temperature {
                date: "2026-07-20".to_owned(),
                hour: 0,
                celsius: 21,
            },
        ]
    );
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
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(u64::try_from(body.len()).expect("test body should fit in u64"));
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .append_data(&mut header, path, body.as_bytes())
        .expect("test archive entry should be written");
    let mut encoder = archive
        .into_inner()
        .expect("test tar archive should finish");
    encoder.flush().expect("test gzip stream should flush");
    encoder.finish().expect("test gzip stream should finish")
}

fn test_client(server: &mockito::Server, retry_base_delay: Duration) -> AemetClient {
    let base_url = Url::parse(&format!("{}/", server.url())).expect("mock URL should parse");
    AemetClient::with_base_url("test-key".to_owned(), base_url, retry_base_delay)
        .expect("test client should build")
}
