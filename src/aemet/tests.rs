use std::{
    io::{Cursor, Write},
    time::Duration,
};

use flate2::{Compression, write::GzEncoder};
use jiff::{Timestamp, civil::date};
use reqwest::Url;
use serde_json::json;

use super::{
    beaches::parse_beach,
    client::{
        AemetClient, MAX_ATTEMPTS, PRODUCT_ATTEMPTS, RequestKind, RetryKind, is_key_error,
        parse_retry_after, redact_url, retry_delay,
    },
    fire::parse_fire_archive,
    forecast::{parse_daily, parse_hourly, resolve_hours},
    models::{Accumulation, MunicipalityTimeZone, WarningLevel},
    sources::{parse_beach_list, parse_master_list},
    tree::xml_to_value,
    warnings::{active_warnings, parse_cap, parse_warning_archive, phenomenon},
};
use crate::catalog::Beach;

fn madrid(local: &str) -> Timestamp {
    format!("{local}[Europe/Madrid]")
        .parse::<jiff::Zoned>()
        .expect("test time should parse")
        .timestamp()
}

#[test]
fn converts_xml_like_the_archive_json() {
    let value = xml_to_value(
        r#"<?xml version="1.0"?><root id="28079"><dia fecha="2026-10-31"><prob_precipitacion periodo="00-24"/><prob_precipitacion periodo="12-24">5</prob_precipitacion><viento periodo="00"><direccion>NE</direccion><velocidad>16</velocidad></viento><uv_max>3</uv_max><nombre>A &amp; B &#241;</nombre></dia></root>"#,
    )
    .expect("XML should convert");

    assert_eq!(
        value,
        json!({
            "id": "28079",
            "dia": {
                "fecha": "2026-10-31",
                "prob_precipitacion": [{"periodo": "00-24"}, {"periodo": "12-24", "valor": "5"}],
                "viento": {"periodo": "00", "direccion": "NE", "velocidad": "16"},
                "uv_max": "3",
                "nombre": "A & B ñ"
            }
        })
    );
}

fn hourly_document() -> serde_json::Value {
    json!({"root": {
        "id": "28079",
        "elaborado": "2026-07-19T08:00:00",
        "nombre": "Madrid",
        "prediccion": {"dia": [{
            "fecha": "2026-07-19T00:00:00",
            "orto": "06:54",
            "ocaso": "21:43",
            "estado_cielo": [
                {"periodo": "10", "valor": "11", "descripcion": "Despejado"},
                {"periodo": "11", "valor": "99n", "descripcion": "Desconocido"},
                {"periodo": "12", "valor": "garbage", "descripcion": ""}
            ],
            "temperatura": [{"periodo": "10", "valor": "27"}, {"periodo": "11", "valor": 29}, {"periodo": "12", "valor": "200"}],
            "sens_termica": {"periodo": "10", "valor": "29"},
            "precipitacion": [{"periodo": "10", "valor": "Ip"}, {"periodo": "11", "valor": 0.4}],
            "nieve": {"periodo": "11", "valor": "1.2"},
            "humedad_relativa": [{"periodo": "10", "valor": 64}, {"periodo": "11", "valor": 101}],
            "viento": {"periodo": "10", "direccion": ["ne"], "velocidad": [18]},
            "racha_max": [{"periodo": "10", "value": "31"}, {"periodo": "11", "valor": ""}],
            "prob_precipitacion": [{"periodo": "0713", "valor": "60"}, {"periodo": "1319", "valor": ""}],
            "prob_tormenta": {"periodo": "0713", "valor": 15},
            "prob_nieve": {"periodo": "0713", "valor": "0"}
        }]}
    }})
}

#[test]
fn parses_every_hourly_field_and_drops_bad_values() {
    let forecast = parse_hourly(&hourly_document(), Some("28079")).expect("forecast should parse");

    let ten = &forecast.hours[&madrid("2026-07-19T10:00")];
    assert_eq!(ten.sky.as_deref(), Some("11"));
    assert_eq!(ten.temperature, Some(27));
    assert_eq!(ten.feels_like, Some(29));
    assert_eq!(ten.precipitation, Some(Accumulation::Trace));
    assert_eq!(ten.humidity, Some(64));
    assert_eq!(ten.wind_direction.as_deref(), Some("NE"));
    assert_eq!(ten.wind_speed, Some(18));
    assert_eq!(ten.gust, Some(31));

    let eleven = &forecast.hours[&madrid("2026-07-19T11:00")];
    assert_eq!(
        eleven.sky.as_deref(),
        Some("99n"),
        "unknown but well-formed codes are kept"
    );
    assert_eq!(
        eleven.precipitation,
        Some(Accumulation::TenthsOfMillimetre(4))
    );
    assert_eq!(eleven.snow, Some(Accumulation::TenthsOfMillimetre(12)));
    assert_eq!(eleven.humidity, None, "out of range values are dropped");
    assert_eq!(eleven.gust, None);

    assert!(
        !forecast.hours.contains_key(&madrid("2026-07-19T12:00")),
        "an hour with only invalid values is dropped"
    );

    assert_eq!(forecast.windows.len(), 1);
    let window = &forecast.windows[0];
    assert_eq!(window.start.timestamp(), madrid("2026-07-19T07:00"));
    assert_eq!(window.end.timestamp(), madrid("2026-07-19T13:00"));
    assert_eq!(
        (window.precipitation, window.storm, window.snow),
        (Some(60), Some(15), Some(0))
    );
    assert_eq!(forecast.sun[0].sunrise.as_deref(), Some("06:54"));
}

#[test]
fn hourly_windows_cross_midnight() {
    let document = json!({"root": {"id": "28079", "prediccion": {"dia": {
        "fecha": "2026-07-19",
        "temperatura": {"periodo": "20", "valor": "25"},
        "prob_precipitacion": {"periodo": "1901", "valor": "10"}
    }}}});
    let forecast = parse_hourly(&document, None).expect("forecast should parse");
    assert_eq!(
        forecast.windows[0].end.timestamp(),
        madrid("2026-07-20T01:00")
    );
}

#[test]
fn rejects_documents_for_another_municipality_or_without_hours() {
    let error =
        parse_hourly(&hourly_document(), Some("28080")).expect_err("ID mismatch should fail");
    assert!(error.to_string().contains("does not match"));
    let empty = json!({"root": {"id": "28079", "prediccion": {"dia": []}}});
    assert!(parse_hourly(&empty, None).is_err());
}

#[test]
fn resolves_repeated_hours_when_clocks_go_back() {
    let zone = MunicipalityTimeZone::EuropeMadrid.rules().expect("zone");
    let times = resolve_hours(
        date(2026, 10, 25),
        &zone,
        &[Some(1), Some(2), Some(2), Some(3)],
    );
    let summer = "2026-10-25T00:00:00Z".parse::<Timestamp>().expect("time");
    let winter = "2026-10-25T01:00:00Z".parse::<Timestamp>().expect("time");
    assert_eq!(times[1], Some(summer));
    assert_eq!(times[2], Some(winter));
    assert_eq!(
        times[3],
        Some("2026-10-25T02:00:00Z".parse().expect("time"))
    );
}

#[test]
fn drops_hours_that_do_not_exist_or_repeat_too_often() {
    let zone = MunicipalityTimeZone::EuropeMadrid.rules().expect("zone");
    let spring = resolve_hours(date(2027, 3, 28), &zone, &[Some(1), Some(2), Some(3)]);
    assert!(spring[0].is_some() && spring[1].is_none() && spring[2].is_some());
    let duplicate = resolve_hours(date(2026, 7, 19), &zone, &[Some(10), Some(10), None]);
    assert_eq!(duplicate[1], None);
    assert_eq!(duplicate[2], None);
}

#[test]
fn hourly_document_published_on_the_day_clocks_go_back_keeps_both_hours() {
    let document = json!({"root": {"id": "28079", "prediccion": {"dia": {
        "fecha": "2026-10-25",
        "temperatura": [{"periodo": "02", "valor": "12"}, {"periodo": "02", "valor": "11"}],
        "estado_cielo": [{"periodo": "02", "valor": "11n"}, {"periodo": "02", "valor": "12n"}]
    }}}});
    let forecast = parse_hourly(&document, None).expect("forecast should parse");
    let values = forecast
        .hours
        .values()
        .map(|hour| (hour.temperature, hour.sky.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [
            (Some(12), Some("11n".to_owned())),
            (Some(11), Some("12n".to_owned()))
        ]
    );
}

const DAILY_XML: &str = r#"<?xml version='1.0' encoding='ISO-8859-15'?>
<root id="28079"><elaborado>2026-10-31T09:49:07</elaborado><prediccion>
<dia fecha="2026-10-31">
  <prob_precipitacion periodo="00-24"/><prob_precipitacion periodo="12-24">0</prob_precipitacion>
  <cota_nieve_prov periodo="12-24"></cota_nieve_prov>
  <estado_cielo periodo="00-24" descripcion=""/><estado_cielo periodo="12-24" descripcion="Despejado">11</estado_cielo>
  <viento periodo="12-24"><direccion>E</direccion><velocidad>5</velocidad></viento>
  <racha_max periodo="12-24">40</racha_max>
  <temperatura><maxima>20</maxima><minima>9</minima><dato hora="06">10</dato><dato hora="12">17</dato></temperatura>
  <sens_termica><maxima>20</maxima><minima>8</minima></sens_termica>
  <humedad_relativa><maxima>80</maxima><minima>50</minima></humedad_relativa>
  <uv_max>3</uv_max>
</dia>
<dia fecha="2026-11-04">
  <prob_precipitacion>45</prob_precipitacion><cota_nieve_prov>1700</cota_nieve_prov>
  <estado_cielo descripcion="Muy nuboso">15</estado_cielo>
  <temperatura><maxima>13</maxima><minima>6</minima></temperatura>
</dia>
</prediccion></root>"#;

#[test]
fn parses_every_daily_field_with_periods() {
    let forecast = parse_daily(&xml_to_value(DAILY_XML).expect("xml"), Some("28079"))
        .expect("daily forecast should parse");

    assert_eq!(forecast.days.len(), 2);
    let first = &forecast.days[0];
    assert_eq!(first.temperature.minimum, Some(9));
    assert_eq!(first.temperature.maximum, Some(20));
    assert_eq!(first.temperature.at_hours.get(&6), Some(&10));
    assert_eq!(first.feels_like.minimum, Some(8));
    assert_eq!(first.humidity.maximum, Some(80));
    assert_eq!(first.uv_index, Some(3));
    assert!(
        !first.periods.contains_key("00-24"),
        "empty periods are dropped"
    );
    let afternoon = &first.periods["12-24"];
    assert_eq!(afternoon.sky.as_deref(), Some("11"));
    assert_eq!(afternoon.precipitation_probability, Some(0));
    assert_eq!(afternoon.wind_direction.as_deref(), Some("E"));
    assert_eq!(afternoon.gust, Some(40));
    assert_eq!(afternoon.snow_level_metres, None);

    let fifth = &forecast.days[1];
    let whole = &fifth.periods["00-24"];
    assert_eq!(whole.sky.as_deref(), Some("15"));
    assert_eq!(whole.precipitation_probability, Some(45));
    assert_eq!(whole.snow_level_metres, Some(1700));
}

const CAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<alert xmlns="urn:oasis:names:tc:emergency:cap:1.2">
  <identifier>ID-1</identifier><status>Actual</status><msgType>Alert</msgType>
  <info>
    <language>es-ES</language><event>Aviso de lluvias de nivel naranja</event>
    <eventCode><valueName>AEMET-Meteoalerta fenomeno</valueName><value>PR;Lluvias</value></eventCode>
    <onset>2026-11-01T13:00:00+01:00</onset><expires>2026-11-02T00:59:59+01:00</expires>
    <headline>Aviso de lluvias de nivel naranja. Tenerife</headline>
    <description>Precipitación acumulada en una hora: 30 mm.</description>
    <instruction>Esté preparado.</instruction>
    <parameter><valueName>AEMET-Meteoalerta nivel</valueName><value>naranja</value></parameter>
    <parameter><valueName>AEMET-Meteoalerta parametro</valueName><value>P1;Precipitación acumulada en una hora;30 mm</value></parameter>
    <parameter><valueName>AEMET-Meteoalerta probabilidad</valueName><value>40%-70%</value></parameter>
    <area><areaDesc>Área metropolitana de Tenerife</areaDesc><polygon>28.4,-16.3 28.5,-16.3 28.5,-16.2 28.4,-16.3</polygon>
      <geocode><valueName>AEMET-Meteoalerta zona</valueName><value>659602</value></geocode></area>
  </info>
  <info>
    <language>en-GB</language><event>Orange rain warning</event>
    <parameter><valueName>AEMET-Meteoalerta nivel</valueName><value>naranja</value></parameter>
    <area><geocode><valueName>AEMET-Meteoalerta zona</valueName><value>659602</value></geocode></area>
  </info>
</alert>"#;

fn cap(
    identifier: &str,
    message_type: &str,
    level: &str,
    expires: &str,
    references: &str,
) -> String {
    CAP.replace(
        "<identifier>ID-1</identifier>",
        &format!("<identifier>{identifier}</identifier><references>{references}</references>"),
    )
    .replace(
        "<msgType>Alert</msgType>",
        &format!("<msgType>{message_type}</msgType>"),
    )
    .replace("<value>naranja</value>", &format!("<value>{level}</value>"))
    .replace("2026-11-02T00:59:59+01:00", expires)
}

#[test]
fn parses_spanish_cap_warnings() {
    let now = "2026-11-01T00:00:00Z".parse::<Timestamp>().expect("time");
    let warnings = active_warnings(vec![parse_cap(CAP).expect("CAP should parse")], now);

    assert_eq!(warnings.len(), 1);
    let warning = &warnings[0];
    assert_eq!(warning.level, WarningLevel::Orange);
    assert_eq!(warning.code.as_deref(), Some("PR"));
    assert_eq!(warning.name, "Lluvias");
    assert_eq!(
        warning.parameter.as_deref(),
        Some("Precipitación acumulada en una hora: 30 mm")
    );
    assert_eq!(warning.probability.as_deref(), Some("40%-70%"));
    assert_eq!(warning.zones, ["659602"]);
    assert_eq!(warning.polygons[0].len(), 4);
    assert_eq!(
        warning.area.as_deref(),
        Some("Área metropolitana de Tenerife")
    );
}

#[test]
fn drops_green_cancelled_superseded_and_expired_warnings() {
    let now = "2026-11-01T00:00:00Z".parse::<Timestamp>().expect("time");
    let later = "2026-11-03T00:00:00+01:00";
    let messages = [
        cap("GREEN", "Alert", "verde", later, ""),
        cap("CANCELLED", "Cancel", "amarillo", later, ""),
        cap("OLD", "Alert", "amarillo", later, ""),
        cap(
            "NEW",
            "Update",
            "rojo",
            later,
            "sender,OLD,2026-10-31T00:00:00+01:00",
        ),
        cap(
            "EXPIRED",
            "Alert",
            "amarillo",
            "2026-10-31T00:00:00+01:00",
            "",
        ),
    ]
    .iter()
    .map(|xml| parse_cap(xml).expect("CAP should parse"))
    .collect();

    let warnings = active_warnings(messages, now);

    assert_eq!(
        warnings
            .iter()
            .map(|warning| warning.id.as_str())
            .collect::<Vec<_>>(),
        ["NEW"]
    );
    assert_eq!(warnings[0].level, WarningLevel::Red);
}

#[test]
fn reads_warning_archives_with_or_without_gzip() {
    let now = "2026-11-01T00:00:00Z".parse::<Timestamp>().expect("time");
    for compressed in [true, false] {
        let archive = archive_entries(
            &[
                ("a.xml", CAP.as_bytes()),
                ("bad.xml", b"<alert>"),
                ("readme.txt", b"x"),
            ],
            compressed,
        );
        let (warnings, failures) =
            parse_warning_archive(&archive, now).expect("archive should parse");
        assert_eq!(warnings.len(), 1);
        assert_eq!(failures.len(), 1);
    }
}

#[test]
fn parses_latin_1_beach_forecasts() {
    let xml = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\" ?><playa id=\"0301101\"><elaborado>2026-10-31T08:51:15</elaborado><prediccion><dia fecha=\"20261031\"><estado_cielo f1=\"130\" descripcion1=\"chubascos\" f2=\"110\" descripcion2=\"nuboso\" /><viento f1=\"220\" f2=\"210\" /><oleaje f1=\"310\" descripcion1=\"d\u{e9}bil\" f2=\"320\" /><t_maxima valor1=\"23\" /><s_termica valor1=\"450\" /><t_agua valor1=\"24\" /><uv_max valor1=\"3\" /></dia></prediccion></playa>";
    let bytes = xml
        .chars()
        .map(|character| u8::try_from(u32::from(character)).expect("latin-1"))
        .collect::<Vec<_>>();
    let beach = Beach {
        id: "0301101".to_owned(),
        name: "Raco de l'Albir".to_owned(),
        municipality_id: "03011".to_owned(),
        latitude: 38.5,
        longitude: -0.06,
    };

    let forecast =
        parse_beach(&super::forecast::decode_document(&bytes), &beach).expect("beach should parse");

    let day = &forecast.days[0];
    assert_eq!(day.date, date(2026, 10, 31));
    assert_eq!(day.sky, [Some(130), Some(110)]);
    assert_eq!(day.waves, [Some(310), Some(320)]);
    assert_eq!(
        (day.water_temperature, day.maximum_temperature, day.uv_index),
        (Some(24), Some(23), Some(3))
    );
    assert!(parse_beach("<playa id=\"999\"/>", &beach).is_err());
}

/// A geographic float `GeoTIFF` with the same layout as AEMET's rasters.
fn fire_raster(west: f64, north: f64, values: &[f32], width: u32) -> Vec<u8> {
    use tiff::{
        encoder::{TiffEncoder, colortype::Gray32Float},
        tags::Tag,
    };
    let height = u32::try_from(values.len()).expect("size") / width;
    let mut bytes = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut bytes).expect("encoder");
    let mut image = encoder
        .new_image::<Gray32Float>(width, height)
        .expect("image");
    image
        .encoder()
        .write_tag(Tag::ModelPixelScaleTag, &[0.01, 0.01, 0.0][..])
        .expect("scale");
    image
        .encoder()
        .write_tag(
            Tag::ModelTiepointTag,
            &[0.0, 0.0, 0.0, west, north, 0.0][..],
        )
        .expect("tiepoint");
    image.write_data(values).expect("data");
    bytes.into_inner()
}

#[test]
fn samples_fire_danger_at_municipality_points() {
    // 4x3 pixels starting at 40.03N 3.72W; the highest level near the point wins.
    let peninsula = fire_raster(
        -3.72,
        40.03,
        &[1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0],
        4,
    );
    let canarias = fire_raster(-18.0, 29.0, &[5.0; 4], 2);
    let archive = archive_entries(
        &[
            ("down_20261030_peligro_p_D00.tif", &peninsula),
            ("down_20261030_peligro_p_D01.tif", &peninsula),
            ("down_20261030_peligro_c_D00.tif", &canarias),
            ("down_20261030_peligro_p_D00.qml", b"style"),
        ],
        true,
    );

    let fire = parse_fire_archive(&archive).expect("fire archive should parse");

    let levels = fire.sample(40.005, -3.705);
    assert_eq!(levels.get(&date(2026, 10, 30)), Some(&3));
    assert_eq!(levels.get(&date(2026, 10, 31)), Some(&3));
    assert!(
        fire.sample(43.0, -8.0).is_empty(),
        "points outside the grid have no level"
    );
    assert_eq!(
        fire.sample(28.995, -17.995).get(&date(2026, 10, 30)),
        Some(&5)
    );
}

fn archive_entries(entries: &[(&str, &[u8])], compressed: bool) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, body) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(body.len()).expect("size"));
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, path, *body)
            .expect("entry");
    }
    let bytes = archive.into_inner().expect("tar");
    if !compressed {
        return bytes;
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&bytes).expect("gzip");
    encoder.finish().expect("gzip")
}

#[tokio::test]
async fn parses_bulk_archives_isolating_bad_documents() {
    let mut server = mockito::Server::new_async().await;
    let good = hourly_document().to_string();
    let archive = archive_entries(
        &[
            ("localidad_h_28079.json", good.as_bytes()),
            ("localidad_h_28080.json", b"{not json"),
            ("localidad_h_28081.json", good.as_bytes()),
        ],
        true,
    );
    let data_url = format!("{}/data", server.url());
    let _envelope = server
        .mock("GET", "/api/prediccion/especifica/municipio/horaria/todos")
        .with_body(format!(
            r#"{{"descripcion":"exito","estado":200,"datos":"{data_url}"}}"#
        ))
        .create_async()
        .await;
    let _data = server
        .mock("GET", "/data")
        .with_body(archive)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let parsed = client
        .hourly_forecasts()
        .await
        .expect("archive should parse");

    assert_eq!(parsed.entries, 3);
    assert_eq!(parsed.items.len(), 1);
    assert_eq!(parsed.items[0].id, "28079");
    let failed = parsed
        .failures
        .iter()
        .map(|failure| failure.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        failed,
        ["28080", "28081"],
        "invalid JSON and mismatched IDs fail only that municipality"
    );
}

#[tokio::test]
async fn explains_rejected_api_keys() {
    let mut server = mockito::Server::new_async().await;
    let _envelope = server
        .mock("GET", "/product")
        .with_status(401)
        .with_body(r#"{"descripcion":"Tu apiKey ha superado el plazo establecido hasta el 15/10/2026 para su renovación.","estado":401}"#)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("expired key should fail");

    assert!(is_key_error(&error));
    let message = format!("{error:#}");
    assert!(message.contains("renovación"));
    assert!(message.contains("AEMET_API_KEY"));
}

#[tokio::test]
async fn treats_no_data_as_empty_for_optional_products() {
    let mut server = mockito::Server::new_async().await;
    let _envelope = server
        .mock("GET", "/product")
        .with_body(r#"{"descripcion":"No hay datos que satisfagan esos criterios","estado":404}"#)
        .expect(2)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    assert_eq!(
        client
            .fetch_parsed_product("product", 1024, |bytes| Ok(bytes.to_vec()))
            .await
            .expect("no data is fine"),
        None
    );
    assert!(client.fetch_product("product", 1024).await.is_err());
}

#[tokio::test]
async fn requests_a_new_data_url_when_one_expired() {
    let mut server = mockito::Server::new_async().await;
    let data_url = format!("{}/data", server.url());
    let envelope = server
        .mock("GET", "/product")
        .with_body(format!(
            r#"{{"descripcion":"exito","estado":200,"datos":"{data_url}"}}"#
        ))
        .expect(2)
        .create_async()
        .await;
    let _expired = server
        .mock("GET", "/data")
        .with_body(r#"{"descripcion":"datos expirados","estado":404}"#)
        .expect(1)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);
    let fresh = server
        .mock("GET", "/data")
        .with_body("payload")
        .create_async()
        .await;

    let data = client
        .fetch_product("product", 1024)
        .await
        .expect("second data URL should work");

    assert_eq!(data, b"payload");
    envelope.assert_async().await;
    fresh.assert_async().await;
}

#[tokio::test]
async fn public_fetches_stay_on_the_public_host() {
    let server = mockito::Server::new_async().await;
    let client = test_client(&server, Duration::ZERO);
    let error = client
        .get_public("https://example.com/xml/playas/x.xml", 1024)
        .await
        .expect_err("other hosts are refused");
    assert!(error.to_string().contains("untrusted"));
}

#[tokio::test]
async fn requires_a_key_for_api_products() {
    let base = Url::parse("https://opendata.example/").expect("url");
    let client =
        AemetClient::with_base_urls(None, base.clone(), base, Duration::ZERO, Duration::ZERO)
            .expect("client");
    let error = client
        .fetch_product("api/prediccion/especifica/municipio/horaria/todos", 1024)
        .await
        .expect_err("no key");
    assert!(error.to_string().contains("AEMET_API_KEY"));
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
        .expect(MAX_ATTEMPTS * PRODUCT_ATTEMPTS)
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
        .expect(PRODUCT_ATTEMPTS)
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

fn test_client(server: &mockito::Server, retry_base_delay: Duration) -> AemetClient {
    let base_url = Url::parse(&format!("{}/", server.url())).expect("mock URL should parse");
    AemetClient::with_base_url("test-key".to_owned(), base_url, retry_base_delay)
        .expect("test client should build")
}

#[tokio::test]
async fn starts_a_fresh_attempt_when_a_download_fails() {
    let mut server = mockito::Server::new_async().await;
    let envelope = format!(r#"{{"estado":200,"datos":"{}/data"}}"#, server.url());
    let envelope_mock = server
        .mock("GET", "/product")
        .with_body(&envelope)
        .expect(2)
        .create_async()
        .await;
    // The first download cannot be parsed (as with a truncated archive), so
    // the product is requested and downloaded again.
    let data_mock = server
        .mock("GET", "/data")
        .with_body("data")
        .expect(2)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);
    let attempts = std::sync::atomic::AtomicUsize::new(0);

    let value = client
        .fetch_parsed_product("product", 1024, |bytes| {
            if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                anyhow::bail!("truncated archive of {} bytes", bytes.len());
            }
            Ok(bytes.len())
        })
        .await
        .expect("second attempt should succeed")
        .expect("data");

    assert_eq!(value, 4);
    data_mock.assert_async().await;
    envelope_mock.assert_async().await;
}

#[tokio::test]
async fn does_not_retry_a_rejected_key() {
    let mut server = mockito::Server::new_async().await;
    let product_mock = server
        .mock("GET", "/product")
        .with_body(r#"{"descripcion":"apiKey no válida","estado":401}"#)
        .expect(1)
        .create_async()
        .await;
    let client = test_client(&server, Duration::ZERO);

    let error = client
        .fetch_product("product", 1024)
        .await
        .expect_err("rejected key");

    assert!(is_key_error(&error));
    product_mock.assert_async().await;
}

#[test]
fn names_warnings_from_the_code_or_the_event_text() {
    assert_eq!(
        phenomenon(Some("TO;Tormentas"), None),
        (Some("TO".to_owned()), "Tormentas".to_owned())
    );
    assert_eq!(
        phenomenon(Some("CO"), Some("Aviso de costeros de nivel amarillo")),
        (Some("CO".to_owned()), "Costeros".to_owned())
    );
    assert_eq!(phenomenon(None, None), (None, "Aviso".to_owned()));
}

#[test]
fn parses_aemets_municipality_list() {
    // A full-size list: AEMET's real one has about 8,100 entries.
    let mut records = (10_000..18_000)
        .map(|code| json!({"id": format!("id{code}"), "nombre": "X"}))
        .collect::<Vec<_>>();
    records.push(json!({
        "id": "id28079", "nombre": "Madrid", "latitud_dec": "40.41", "longitud_dec": "-3.70",
        "altitud": "657", "num_hab": "3165235", "zona_comarcal": "722802"
    }));
    let bytes = serde_json::to_vec(&records).expect("JSON");

    let municipalities = parse_master_list(&bytes).expect("list should parse");
    let madrid = municipalities
        .iter()
        .find(|municipality| municipality.id == "28079")
        .expect("Madrid");
    assert_eq!(madrid.name, "Madrid");
    assert_eq!(madrid.latitude, Some(40.41));
    assert_eq!(madrid.altitude_metres, Some(657));
    assert_eq!(madrid.population, Some(3_165_235));
    assert_eq!(madrid.warning_zone.as_deref(), Some("722802"));
    assert!(
        parse_master_list(b"[]").is_err(),
        "a short list is rejected"
    );
}

#[test]
fn parses_aemets_beach_list() {
    let csv = "ID_PLAYA;NOMBRE_PLAYA;ID_PROVINCIA;NOMBRE_PROVINCIA;ID_MUNICIPIO;NOMBRE_MUNICIPIO;LATITUD;LONGITUD\n\
               0301401;Sant  Joan / San Juan;3;Alicante;3014;Alacant/Alicante;38º 22' 48\";-0º 24' 32\"\n";
    let beaches = parse_beach_list(csv.as_bytes()).expect("list should parse");
    assert_eq!(beaches.len(), 1);
    let beach = &beaches[0];
    assert_eq!(beach.id, "0301401");
    assert_eq!(beach.name, "Sant Joan / San Juan");
    assert_eq!(beach.municipality_id, "03014");
    assert!((beach.latitude - 38.38).abs() < 0.001);
    assert!((beach.longitude + 0.408_888).abs() < 0.001);
}
