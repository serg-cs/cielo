//! Meteoalerta weather warnings in CAP 1.2 format.

use std::collections::HashSet;

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde_json::Value;
use tracing::warn;

use super::{
    archive::{MAX_DOCUMENT_ENTRY, file_name, read_archive},
    forecast::decode_document,
    models::{AemetProduct, EntryFailure, WarningLevel, WeatherWarning},
    tree::{field_text, list, xml_to_value},
};

const ZONE_GEOCODE: &str = "AEMET-Meteoalerta zona";
const LEVEL_PARAMETER: &str = "AEMET-Meteoalerta nivel";
const THRESHOLD_PARAMETER: &str = "AEMET-Meteoalerta parametro";
const PROBABILITY_PARAMETER: &str = "AEMET-Meteoalerta probabilidad";
const PHENOMENON_CODE: &str = "AEMET-Meteoalerta fenomeno";

/// One parsed CAP message.
#[derive(Debug)]
pub(super) struct CapMessage {
    identifier: String,
    message_type: String,
    references: Vec<String>,
    warnings: Vec<WeatherWarning>,
}

/// Parse an archive (tar or tar.gz) of CAP files into active warnings.
pub(super) fn parse_warning_archive(
    bytes: &[u8],
    now: Timestamp,
) -> Result<(Vec<WeatherWarning>, Vec<EntryFailure>)> {
    let entries = read_archive(bytes, "warnings", MAX_DOCUMENT_ENTRY)?;
    let mut messages = Vec::new();
    let mut failures = Vec::new();
    for entry in entries {
        if !entry.path.to_ascii_lowercase().ends_with(".xml") {
            continue;
        }
        match parse_cap(&decode_document(&entry.body)) {
            Ok(message) => messages.push(message),
            Err(error) => failures.push(EntryFailure {
                id: file_name(&entry.path).to_owned(),
                product: AemetProduct::Warnings,
                error,
            }),
        }
    }
    Ok((active_warnings(messages, now), failures))
}

/// Parse one CAP document.
pub(super) fn parse_cap(xml: &str) -> Result<CapMessage> {
    let document = xml_to_value(xml).context("invalid CAP XML")?;
    let alert = document.get("alert").unwrap_or(&document);
    let identifier = field_text(alert, "identifier").context("CAP alert has no identifier")?;
    let message_type = field_text(alert, "msgType").unwrap_or_else(|| "Alert".to_owned());
    let status = field_text(alert, "status").unwrap_or_else(|| "Actual".to_owned());
    let references = field_text(alert, "references")
        .map(|references| {
            references
                .split_whitespace()
                .filter_map(|reference| reference.split(',').nth(1).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let infos = list(alert, "info");
    let spanish = infos
        .iter()
        .copied()
        .filter(|info| {
            field_text(info, "language").is_none_or(|language| language.starts_with("es"))
        })
        .collect::<Vec<_>>();
    let infos = if spanish.is_empty() { infos } else { spanish };

    let mut warnings = Vec::new();
    if status.eq_ignore_ascii_case("actual") {
        for (index, info) in infos.iter().enumerate() {
            if let Some(warning) = parse_info(info, &identifier, index) {
                warnings.push(warning);
            }
        }
    }
    Ok(CapMessage {
        identifier,
        message_type,
        references,
        warnings,
    })
}

fn parse_info(info: &Value, identifier: &str, index: usize) -> Option<WeatherWarning> {
    let level = match named_value(info, "parameter", LEVEL_PARAMETER)?
        .to_lowercase()
        .as_str()
    {
        "amarillo" => WarningLevel::Yellow,
        "naranja" => WarningLevel::Orange,
        "rojo" => WarningLevel::Red,
        _ => return None,
    };
    let (code, name) = phenomenon(
        named_value(info, "eventCode", PHENOMENON_CODE).as_deref(),
        field_text(info, "event").as_deref(),
    );
    let parameter = named_value(info, "parameter", THRESHOLD_PARAMETER).map(|value| {
        let parts = value
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        match parts.as_slice() {
            [_, name, threshold] => format!("{name}: {threshold}"),
            [_, name] => (*name).to_owned(),
            _ => value.clone(),
        }
    });

    let mut zones = Vec::new();
    let mut polygons = Vec::new();
    let mut area_names = Vec::new();
    for area in list(info, "area") {
        if let Some(name) = field_text(area, "areaDesc") {
            area_names.push(name);
        }
        for geocode in list(area, "geocode") {
            if field_text(geocode, "valueName").as_deref() == Some(ZONE_GEOCODE)
                && let Some(zone) = field_text(geocode, "value")
                && !zones.contains(&zone)
            {
                zones.push(zone);
            }
        }
        for polygon in list(area, "polygon") {
            if let Some(points) = field_text_of(polygon).map(|text| parse_polygon(&text))
                && points.len() >= 3
            {
                polygons.push(points);
            }
        }
    }
    if zones.is_empty() {
        return None;
    }

    Some(WeatherWarning {
        id: if index == 0 {
            identifier.to_owned()
        } else {
            format!("{identifier}#{index}")
        },
        level,
        code,
        name,
        onset: timestamp(field_text(info, "onset").or_else(|| field_text(info, "effective"))),
        expires: timestamp(field_text(info, "expires")),
        description: field_text(info, "description"),
        parameter,
        probability: named_value(info, "parameter", PROBABILITY_PARAMETER),
        zones,
        area: (!area_names.is_empty()).then(|| area_names.join(", ")),
        polygons,
    })
}

/// Phenomenon code and Spanish name from AEMET's `PR;Lluvias`, or from the
/// event text (`Aviso de lluvias de nivel amarillo`) when the name is missing.
pub(super) fn phenomenon(
    event_code: Option<&str>,
    event: Option<&str>,
) -> (Option<String>, String) {
    let mut parts = event_code.unwrap_or_default().split(';').map(str::trim);
    let code = parts
        .next()
        .filter(|code| !code.is_empty())
        .map(str::to_owned);
    let name = parts
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let event = event?.trim();
            let rest = event
                .strip_prefix("Aviso de ")
                .or_else(|| event.strip_prefix("Aviso por "))?;
            let name = rest.split(" de nivel").next()?.trim();
            let mut characters = name.chars();
            let first = characters.next()?;
            Some(first.to_uppercase().chain(characters).collect())
        })
        .unwrap_or_else(|| "Aviso".to_owned());
    (code, name)
}

fn field_text_of(value: &Value) -> Option<String> {
    super::tree::text(value)
}

/// Value of the `valueName`/`value` pair named `name` under `key`.
fn named_value(info: &Value, key: &str, name: &str) -> Option<String> {
    list(info, key)
        .into_iter()
        .find(|pair| field_text(pair, "valueName").as_deref() == Some(name))
        .and_then(|pair| field_text(pair, "value"))
}

fn timestamp(value: Option<String>) -> Option<Timestamp> {
    value?.parse::<Timestamp>().ok()
}

/// CAP polygon: `lat,lon lat,lon ...`.
fn parse_polygon(text: &str) -> Vec<(f64, f64)> {
    text.split_whitespace()
        .filter_map(|pair| {
            let (latitude, longitude) = pair.split_once(',')?;
            Some((latitude.parse().ok()?, longitude.parse().ok()?))
        })
        .filter(|(latitude, longitude): &(f64, f64)| latitude.is_finite() && longitude.is_finite())
        .collect()
}

/// Keep current warnings: drop cancelled, superseded and expired messages.
pub(super) fn active_warnings(messages: Vec<CapMessage>, now: Timestamp) -> Vec<WeatherWarning> {
    let superseded = messages
        .iter()
        .flat_map(|message| message.references.iter().cloned())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();
    for message in messages {
        if message.message_type.eq_ignore_ascii_case("cancel")
            || superseded.contains(&message.identifier)
        {
            continue;
        }
        for warning in message.warnings {
            if warning.expires.is_some_and(|expires| expires <= now) {
                continue;
            }
            if seen.insert(warning.id.clone()) {
                warnings.push(warning);
            } else {
                warn!(id = %warning.id, "skipping duplicate warning");
            }
        }
    }
    warnings.sort_by(|left, right| {
        right
            .level
            .cmp(&left.level)
            .then(left.onset.cmp(&right.onset))
            .then(left.id.cmp(&right.id))
    });
    warnings
}

/// Links to CAP files listed in AEMET's public warnings RSS feed.
pub(super) fn rss_links(xml: &str) -> Result<Vec<String>> {
    let document = xml_to_value(xml).context("invalid warnings RSS")?;
    let channel = document
        .get("channel")
        .context("warnings RSS has no channel")?;
    Ok(list(channel, "item")
        .into_iter()
        .filter_map(|item| field_text(item, "link"))
        .filter(|link| link.starts_with("https://www.aemet.es/"))
        .collect())
}
