pub(super) fn decode_iso_8859_15(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0xA4 => '\u{20AC}',
            0xA6 => '\u{0160}',
            0xA8 => '\u{0161}',
            0xB4 => '\u{017D}',
            0xB8 => '\u{017E}',
            0xBC => '\u{0152}',
            0xBD => '\u{0153}',
            0xBE => '\u{0178}',
            value => char::from(*value),
        })
        .collect()
}

pub(super) fn repair_iso_8859_15_mojibake(value: &str) -> String {
    let Some(bytes) = encode_iso_8859_15(value) else {
        return value.to_owned();
    };

    String::from_utf8(bytes).unwrap_or_else(|_| value.to_owned())
}

fn encode_iso_8859_15(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(value.len());
    for character in value.chars() {
        let byte = match character {
            '\u{20AC}' => 0xA4,
            '\u{0160}' => 0xA6,
            '\u{0161}' => 0xA8,
            '\u{017D}' => 0xB4,
            '\u{017E}' => 0xB8,
            '\u{0152}' => 0xBC,
            '\u{0153}' => 0xBD,
            '\u{0178}' => 0xBE,
            '\u{00A4}' | '\u{00A6}' | '\u{00A8}' | '\u{00B4}' | '\u{00B8}' | '\u{00BC}'
            | '\u{00BD}' | '\u{00BE}' => return None,
            _ => u8::try_from(u32::from(character)).ok()?,
        };
        bytes.push(byte);
    }
    Some(bytes)
}
