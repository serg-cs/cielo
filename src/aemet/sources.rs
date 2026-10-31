//! AEMET's lists of places, used to refresh the catalog: the municipality
//! master list (API) and the beach list (public CSV).

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::{
    decoding::decode_iso_8859_15,
    tree::{field_integer, field_text, number},
};
use crate::catalog::{Beach, Municipality, validate_municipality_id};

/// Parse `maestro/municipios`. Names are AEMET's, not display names.
pub(super) fn parse_master_list(bytes: &[u8]) -> Result<Vec<Municipality>> {
    let text = decode_iso_8859_15(bytes);
    let records: Vec<Value> =
        serde_json::from_str(&text).context("invalid municipality list JSON")?;
    let mut seen = HashSet::new();
    let mut municipalities = Vec::with_capacity(records.len());
    for record in &records {
        let Some(id) = field_text(record, "id").map(|id| id.trim_start_matches("id").to_owned())
        else {
            continue;
        };
        if validate_municipality_id(&id).is_err() {
            continue;
        }
        if !seen.insert(id.clone()) {
            bail!("municipality {id} appears twice in AEMET's list");
        }
        municipalities.push(Municipality {
            name: field_text(record, "nombre")
                .unwrap_or_default()
                .trim()
                .to_owned(),
            latitude: record
                .get("latitud_dec")
                .and_then(number)
                .filter(|value| (25.0..=45.0).contains(value)),
            longitude: record
                .get("longitud_dec")
                .and_then(number)
                .filter(|value| (-20.0..=6.0).contains(value)),
            altitude_metres: field_integer(record, "altitud", -100, 5000)
                .and_then(|value| i32::try_from(value).ok()),
            population: field_integer(record, "num_hab", 0, 10_000_000)
                .and_then(|value| u32::try_from(value).ok()),
            warning_zone: field_text(record, "zona_comarcal")
                .filter(|zone| zone.len() == 6 && zone.bytes().all(|byte| byte.is_ascii_digit())),
            id,
        });
    }
    if municipalities.len() < 8_000 {
        bail!(
            "AEMET's municipality list has only {} entries",
            municipalities.len()
        );
    }
    Ok(municipalities)
}

/// Parse `Playas_codigos.csv`: semicolon separated, Latin-1, coordinates in
/// degrees, minutes and seconds.
pub(super) fn parse_beach_list(bytes: &[u8]) -> Result<Vec<Beach>> {
    let text = decode_iso_8859_15(bytes);
    let mut lines = text.lines();
    let header = lines
        .next()
        .context("the beach list is empty")?
        .split(';')
        .map(|field| field.trim().trim_start_matches('\u{feff}').to_owned())
        .collect::<Vec<_>>();
    let column = |name: &str| {
        header
            .iter()
            .position(|field| field == name)
            .with_context(|| format!("the beach list has no {name} column"))
    };
    let (id, name, municipality, latitude, longitude) = (
        column("ID_PLAYA")?,
        column("NOMBRE_PLAYA")?,
        column("ID_MUNICIPIO")?,
        column("LATITUD")?,
        column("LONGITUD")?,
    );
    let mut beaches = Vec::new();
    for (index, line) in lines
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
    {
        let fields = line.split(';').map(str::trim).collect::<Vec<_>>();
        let field = |position: usize| fields.get(position).copied().unwrap_or_default();
        let context = || format!("beach list line {}", index + 2);
        let municipality_id = format!("{:0>5}", field(municipality));
        validate_municipality_id(&municipality_id).with_context(context)?;
        let beach_id = field(id).to_owned();
        if beach_id.len() != 7 || !beach_id.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("{}: invalid beach code {beach_id}", context());
        }
        beaches.push(Beach {
            id: beach_id,
            name: field(name).split_whitespace().collect::<Vec<_>>().join(" "),
            municipality_id,
            latitude: degrees(field(latitude)).with_context(context)?,
            longitude: degrees(field(longitude)).with_context(context)?,
        });
    }
    Ok(beaches)
}

/// `-3º 42' 15,5"` and similar forms to decimal degrees.
fn degrees(value: &str) -> Result<f64> {
    let negative = value.trim_start().starts_with('-');
    let parts = value
        .split(|character: char| {
            !(character.is_ascii_digit() || character == ',' || character == '.')
        })
        .filter(|part| !part.is_empty())
        .map(|part| part.replace(',', ".").parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("invalid coordinate {value:?}"))?;
    let [degrees, minutes, seconds] = parts[..] else {
        bail!("invalid coordinate {value:?}");
    };
    let decimal = degrees + minutes / 60.0 + seconds / 3600.0;
    Ok(if negative { -decimal } else { decimal })
}
