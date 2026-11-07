//! Beach forecasts from AEMET's public `play_v2` XML files.

use anyhow::{Result, bail};
use serde_json::Value;

use super::{
    forecast::parse_date,
    models::{BeachDay, BeachForecast},
    tree::{field_integer, field_text, list, xml_to_value},
};
use crate::catalog::Beach;

/// Parse one beach XML document.
pub(super) fn parse_beach(xml: &str, beach: &Beach) -> Result<BeachForecast> {
    let document = xml_to_value(xml)?;
    let root = document.get("playa").unwrap_or(&document);
    if let Some(id) = field_text(root, "id")
        && id != beach.id
    {
        bail!(
            "beach document {id} does not match requested beach {}",
            beach.id
        );
    }
    let days = root
        .get("prediccion")
        .map(|prediction| list(prediction, "dia"))
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_day)
        .collect::<Vec<_>>();
    if days.is_empty() {
        bail!("beach {} forecast does not contain any days", beach.id);
    }
    Ok(BeachForecast {
        id: beach.id.clone(),
        name: beach.name.clone(),
        municipality_id: beach.municipality_id.clone(),
        days,
    })
}

fn parse_day(day: &Value) -> Option<BeachDay> {
    let date = parse_date(&field_text(day, "fecha")?)?;
    let pair = |key: &str| {
        let Some(item) = day.get(key) else {
            return [None, None];
        };
        let code =
            |field| field_integer(item, field, 0, 999).and_then(|code| u16::try_from(code).ok());
        [code("f1"), code("f2")]
    };
    let single = |key: &str, min, max| {
        day.get(key)
            .and_then(|item| field_integer(item, "valor1", min, max))
    };
    Some(BeachDay {
        date,
        sky: pair("estado_cielo"),
        wind: pair("viento"),
        waves: pair("oleaje"),
        maximum_temperature: single("t_maxima", -30, 60)
            .and_then(|value| i16::try_from(value).ok()),
        water_temperature: single("t_agua", -5, 40).and_then(|value| i16::try_from(value).ok()),
        feels_like: single("s_termica", 0, 999).and_then(|value| u16::try_from(value).ok()),
        uv_index: single("uv_max", 0, 20).and_then(|value| u8::try_from(value).ok()),
    })
}
