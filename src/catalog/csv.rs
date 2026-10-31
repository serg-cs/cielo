//! The small CSV dialect of the catalog files: comma separated, a header
//! row, fields quoted with `"` when they contain a comma or a quote.

use anyhow::{Result, bail};

/// Rows of a CSV file whose header must be exactly `header`.
pub(super) fn read(source: &str, header: &[&str], file: &str) -> Result<Vec<Vec<String>>> {
    let mut lines = source.lines();
    match lines.next().map(parse_line).transpose()? {
        Some(found) if found == header => {}
        _ => bail!("{file}: the header must be `{}`", header.join(",")),
    }
    lines
        .enumerate()
        .map(|(index, line)| {
            let fields = parse_line(line)
                .map_err(|error| anyhow::anyhow!("{file} line {}: {error}", index + 2))?;
            if fields.len() != header.len() {
                bail!(
                    "{file} line {}: expected {} fields, found {}",
                    index + 2,
                    header.len(),
                    fields.len()
                );
            }
            Ok(fields)
        })
        .collect()
}

/// Render rows as CSV with a header, quoting only where needed.
pub(super) fn write(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut output = String::new();
    for row in std::iter::once(header.iter().map(|field| (*field).to_owned()).collect())
        .chain(rows.iter().cloned())
    {
        let fields = row.iter().map(|field| quote(field)).collect::<Vec<_>>();
        output.push_str(&fields.join(","));
        output.push('\n');
    }
    output
}

fn quote(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Split one line into fields, honouring quotes.
pub(super) fn parse_line(line: &str) -> Result<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut characters = line.chars().peekable();
    let mut quoted = false;
    let mut after_quote = false;
    while let Some(character) = characters.next() {
        if quoted {
            match character {
                '"' if characters.peek() == Some(&'"') => {
                    characters.next();
                    field.push('"');
                }
                '"' => {
                    quoted = false;
                    after_quote = true;
                }
                _ => field.push(character),
            }
        } else if character == ',' {
            fields.push(std::mem::take(&mut field));
            after_quote = false;
        } else if after_quote {
            bail!("unexpected text after a closing quote");
        } else if character == '"' && field.is_empty() {
            quoted = true;
        } else {
            field.push(character);
        }
    }
    if quoted {
        bail!("unterminated quoted field");
    }
    fields.push(field);
    Ok(fields)
}
