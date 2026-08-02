use std::collections::HashMap;

use anyhow::{Context, Result, bail};

const MUNICIPALITY_NAMES: &str = include_str!("municipalities.csv");
const PROVINCE_NAMES: &str = include_str!("provinces.csv");

pub(super) struct LocationNames {
    municipalities: HashMap<String, String>,
    provinces: HashMap<String, String>,
}

impl LocationNames {
    pub(super) fn load() -> Result<Self> {
        let municipalities = parse_catalog(MUNICIPALITY_NAMES, 5, "municipality")?;
        let provinces = parse_catalog(PROVINCE_NAMES, 2, "province")?;

        Ok(Self {
            municipalities,
            provinces,
        })
    }

    pub(super) fn municipality(&self, municipality_id: &str) -> Result<&str> {
        self.municipalities
            .get(municipality_id)
            .map(String::as_str)
            .with_context(|| format!("missing Spanish name for municipality {municipality_id}"))
    }

    pub(super) fn province(&self, municipality_id: &str) -> Result<&str> {
        let province_id = municipality_id
            .get(..2)
            .with_context(|| format!("invalid municipality ID: {municipality_id}"))?;

        self.provinces
            .get(province_id)
            .map(String::as_str)
            .with_context(|| format!("missing Spanish name for province {province_id}"))
    }
}

fn parse_catalog(
    source: &str,
    code_length: usize,
    location_kind: &str,
) -> Result<HashMap<String, String>> {
    let mut names = HashMap::with_capacity(source.lines().count());
    let mut lines = source.lines().enumerate();
    let Some((_, header)) = lines.next() else {
        bail!("{location_kind} catalog is empty");
    };
    if parse_csv_row(header)? != ["code", "name"] {
        bail!("invalid {location_kind} catalog header");
    }

    for (line_index, line) in lines {
        let line_number = line_index + 1;
        let fields = parse_csv_row(line)
            .with_context(|| format!("invalid {location_kind} catalog line {line_number}"))?;
        let [code, name]: [String; 2] = fields.try_into().map_err(|fields: Vec<String>| {
            anyhow::anyhow!(
                "invalid {location_kind} catalog line {line_number}: expected 2 columns, found {}",
                fields.len()
            )
        })?;

        if code.len() != code_length || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("invalid {location_kind} code on catalog line {line_number}: {code}");
        }
        if name.is_empty() || name.trim() != name {
            bail!("invalid {location_kind} name on catalog line {line_number}");
        }
        if name.contains('/') || name.chars().any(char::is_control) {
            bail!("non-display {location_kind} name on catalog line {line_number}: {name}");
        }
        if names.insert(code.clone(), name).is_some() {
            bail!("duplicate {location_kind} code in catalog: {code}");
        }
    }

    if names.is_empty() {
        bail!("{location_kind} catalog is empty");
    }

    Ok(names)
}

fn parse_csv_row(line: &str) -> Result<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut characters = line.chars().peekable();
    let mut quoted = false;
    let mut closed_quote = false;
    let mut field_start = true;

    while let Some(character) = characters.next() {
        if quoted {
            if character != '"' {
                field.push(character);
            } else if characters.peek() == Some(&'"') {
                characters.next();
                field.push('"');
            } else {
                quoted = false;
                closed_quote = true;
            }
        } else if closed_quote {
            if character != ',' {
                bail!("unexpected character after closing quote");
            }
            fields.push(std::mem::take(&mut field));
            closed_quote = false;
            field_start = true;
        } else if character == ',' {
            fields.push(std::mem::take(&mut field));
            field_start = true;
        } else if character == '"' {
            if !field_start {
                bail!("unexpected quote in unquoted field");
            }
            quoted = true;
            field_start = false;
        } else {
            field.push(character);
            field_start = false;
        }
    }

    if quoted {
        bail!("unclosed quoted field");
    }
    fields.push(field);

    Ok(fields)
}

#[cfg(test)]
mod tests;
