//! Tolerant access to AEMET documents.
//!
//! AEMET publishes the same products as JSON (inside the bulk archives) and as
//! XML (public per-municipality files). Values may be numbers or strings, a
//! single object or a list, and empty elements are common. Everything here
//! returns `None` instead of failing so one odd value never drops a whole
//! municipality.

use anyhow::{Context, Result, bail};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};
use serde_json::{Map, Value};

/// Key used for an element's text when it also has attributes or children.
pub(super) const TEXT_KEY: &str = "valor";

/// Children of `value` under `key`, as a list whatever the source shape.
pub(super) fn list<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    match value.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(item) => vec![item],
    }
}

/// Trimmed text of a scalar or of an object's `valor`/`value`, if not empty.
pub(super) fn text(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Number(number) => number.to_string(),
        Value::Object(map) => {
            return map
                .get(TEXT_KEY)
                .or_else(|| map.get("value"))
                .and_then(text);
        }
        Value::Array(items) => return items.first().and_then(text),
        Value::Bool(_) | Value::Null => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// Text of the field `key`.
pub(super) fn field_text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(text)
}

/// A number, accepting a decimal comma.
pub(super) fn number(value: &Value) -> Option<f64> {
    if let Value::Number(number) = value {
        return number.as_f64();
    }
    let text = text(value)?;
    text.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// A whole number in `min..=max`; decimals are rounded.
pub(super) fn integer(value: &Value, min: i64, max: i64) -> Option<i64> {
    let number = number(value)?;
    #[allow(clippy::cast_possible_truncation)]
    let rounded = number.round() as i64;
    (min..=max).contains(&rounded).then_some(rounded)
}

pub(super) fn field_integer(value: &Value, key: &str, min: i64, max: i64) -> Option<i64> {
    value.get(key).and_then(|value| integer(value, min, max))
}

/// Convert an XML document into the JSON shape used by AEMET's archives.
///
/// - Attributes become string fields.
/// - Child elements become fields; repeated children become a list.
/// - Text becomes the element value, or its `valor` field when the element
///   also has attributes or children.
pub(super) fn xml_to_value(xml: &str) -> Result<Value> {
    let mut reader = Reader::from_str(xml);

    // Each open element: its name, fields, and accumulated text.
    let mut stack: Vec<(String, Map<String, Value>, String)> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().context("invalid XML")? {
            Event::Start(start) => stack.push(open_element(&start)?),
            Event::Empty(start) => {
                let (name, fields, text) = open_element(&start)?;
                close_element(&mut stack, &mut root, name, fields, &text)?;
            }
            Event::Text(content) => {
                if let Some((_, _, text)) = stack.last_mut() {
                    text.push_str(&content.xml10_content());
                }
            }
            Event::CData(content) => {
                if let Some((_, _, text)) = stack.last_mut() {
                    text.push_str(&content.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                if let Some((_, _, text)) = stack.last_mut() {
                    if let Some(character) = reference
                        .resolve_char_ref()
                        .context("invalid XML character reference")?
                    {
                        text.push(character);
                    } else {
                        let name = reference.xml10_content();
                        let resolved = quick_xml::escape::resolve_predefined_entity(&name)
                            .with_context(|| format!("unknown XML entity &{name};"))?;
                        text.push_str(resolved);
                    }
                }
            }
            Event::End(_) => {
                let (name, fields, text) = stack.pop().context("unbalanced XML")?;
                close_element(&mut stack, &mut root, name, fields, &text)?;
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        bail!("XML document ended inside an element");
    }
    root.context("XML document has no root element")
}

fn open_element(start: &BytesStart<'_>) -> Result<(String, Map<String, Value>, String)> {
    let name = local_name(start.name().as_ref());
    let mut fields = Map::new();
    for attribute in start.attributes() {
        let attribute = attribute.context("invalid XML attribute")?;
        let key = local_name(attribute.key.as_ref());
        if attribute.key.as_ref().starts_with("xmlns") {
            continue;
        }
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .context("invalid XML attribute value")?
            .trim()
            .to_owned();
        fields.insert(key, Value::String(value));
    }
    Ok((name, fields, String::new()))
}

fn close_element(
    stack: &mut [(String, Map<String, Value>, String)],
    root: &mut Option<Value>,
    name: String,
    mut fields: Map<String, Value>,
    text: &str,
) -> Result<()> {
    let text = text.trim().to_owned();
    let value = if fields.is_empty() {
        Value::String(text)
    } else {
        if !text.is_empty() {
            fields.insert(TEXT_KEY.to_owned(), Value::String(text));
        }
        Value::Object(fields)
    };

    let Some((_, parent, _)) = stack.last_mut() else {
        if root.replace(value).is_some() {
            bail!("XML document has more than one root element");
        }
        return Ok(());
    };
    match parent.get_mut(&name) {
        None => {
            parent.insert(name, value);
        }
        Some(Value::Array(items)) => items.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
    }
    Ok(())
}

fn local_name(name: &str) -> String {
    match name.rsplit_once(':') {
        Some((_, local)) => local.to_owned(),
        None => name.to_owned(),
    }
}
