//! Java Properties byte-stream decoding for loader metadata, including escapes and continuations.

use std::collections::HashMap;

use anyhow::{Context, Result};

/// Matches Properties.load(InputStream): ISO-8859-1 bytes and last declaration wins.
pub(crate) fn parse(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    let mut cursor = 0;
    let mut line_number = 0;
    let mut first_line = 0;
    let mut logical = Vec::new();
    let mut continued = false;
    while cursor < bytes.len() {
        let start = cursor;
        while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
            cursor += 1;
        }
        let mut line = &bytes[start..cursor];
        if bytes.get(cursor) == Some(&b'\r') {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        line_number += 1;
        line = &line[line.iter().take_while(|byte| whitespace(byte)).count()..];
        if !continued {
            if line.is_empty() || matches!(line[0], b'#' | b'!') {
                continue;
            }
            first_line = line_number;
        }
        logical.extend_from_slice(line);
        continued = line.iter().rev().take_while(|byte| **byte == b'\\').count() % 2 == 1;
        if continued {
            logical.pop();
        } else {
            insert(&mut values, &logical)
                .with_context(|| format!("invalid Java property at line {first_line}"))?;
            logical.clear();
        }
    }
    if continued {
        insert(&mut values, &logical)
            .with_context(|| format!("invalid Java property at line {first_line}"))?;
    }
    Ok(values)
}

fn insert(values: &mut HashMap<String, String>, line: &[u8]) -> Result<()> {
    let mut end = 0;
    while end < line.len() {
        match line[end] {
            b'\\' => end = (end + 2).min(line.len()),
            b'=' | b':' => break,
            byte if whitespace(&byte) => break,
            _ => end += 1,
        }
    }
    let mut value = end;
    while value < line.len() && whitespace(&line[value]) {
        value += 1;
    }
    if matches!(line.get(value), Some(b'=' | b':')) {
        value += 1;
    }
    while value < line.len() && whitespace(&line[value]) {
        value += 1;
    }
    values.insert(decode(&line[..end])?, decode(&line[value..])?);
    Ok(())
}

fn whitespace(byte: &u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0c)
}

fn decode(bytes: &[u8]) -> Result<String> {
    let mut chars = Vec::<u16>::new();
    let mut cursor = 0;
    while let Some(&byte) = bytes.get(cursor) {
        cursor += 1;
        if byte != b'\\' {
            chars.push(u16::from(byte));
            continue;
        }
        let Some(&escaped) = bytes.get(cursor) else {
            break;
        };
        cursor += 1;
        chars.push(match escaped {
            b't' => u16::from(b'\t'),
            b'n' => u16::from(b'\n'),
            b'r' => u16::from(b'\r'),
            b'f' => 0x0c,
            b'u' => {
                let digits = bytes
                    .get(cursor..cursor + 4)
                    .context("incomplete Unicode escape in Java property")?;
                let mut value = 0;
                for digit in digits {
                    let hex = char::from(*digit)
                        .to_digit(16)
                        .context("invalid Unicode escape in Java property")?;
                    value = (value << 4) | hex as u16;
                }
                cursor += 4;
                value
            }
            other => u16::from(other),
        });
    }
    // Rust metadata strings cannot represent Java's unpaired UTF-16 surrogates.
    String::from_utf16(&chars).context("unpaired UTF-16 surrogate in Java property")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_lines_separators_and_utf16_match_java_properties() {
        let values = parse(b"a=old\na:new\nempty\n=x\nspace\\ key : =value  \ncontinued=hello\\\n  #part\\\r\n world\nescaped\\u003dkey=\\uD83D\\uDE00\ntrailing=abc\\").unwrap();
        for (key, value) in [
            ("", "x"),
            ("a", "new"),
            ("empty", ""),
            ("space key", "=value  "),
            ("continued", "hello#partworld"),
            ("escaped=key", "😀"),
            ("trailing", "abc"),
        ] {
            assert_eq!(values.get(key).map(String::as_str), Some(value), "{key}");
        }
        assert!(parse(br"broken=\u12xz").is_err());
    }
}
