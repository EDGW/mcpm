//! The single Java-properties codec: select keys without interpreting unrelated values or comments.

use anyhow::{Context, Result, bail};
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

/// Selected raw values are decoded lazily, so disabled features do not validate unused settings.
pub struct Properties {
    values: HashMap<String, Value>,
}
struct Value {
    bytes: Vec<u8>,
    line: usize,
}
impl Properties {
    pub fn read_selected(path: &Path, keys: &[&str]) -> Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read properties {path:?}"));
            }
        };
        let mut reader = BufReader::new(file);
        let mut physical = Vec::new();
        let mut logical = Vec::new();
        let mut values = HashMap::new();
        let mut continuing = false;
        let mut line = 0;
        let mut start = 0;
        while physical_line(&mut reader, &mut physical)? {
            line += 1;
            let content = &physical[physical
                .iter()
                .position(|byte| !space(*byte))
                .unwrap_or(physical.len())..];
            if !continuing {
                if content.is_empty() || matches!(content[0], b'#' | b'!') {
                    continue;
                }
                logical.clear();
                start = line;
            }
            logical.extend_from_slice(content);
            continuing = logical
                .iter()
                .rev()
                .take_while(|byte| **byte == b'\\')
                .count()
                % 2
                == 1;
            if continuing {
                logical.pop();
                continue;
            }
            select(&logical, start, keys, &mut values);
        }
        if continuing {
            select(&logical, start, keys, &mut values);
        }
        Ok(Some(Self { values }))
    }
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.values
            .get(key)
            .map(|value| {
                decode(&value.bytes).with_context(|| {
                    format!("invalid properties value for {key} at line {}", value.line)
                })
            })
            .transpose()
    }
}
fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0c)
}
fn physical_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool> {
    line.clear();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return Ok(!line.is_empty());
        }
        if let Some(end) = bytes.iter().position(|byte| matches!(*byte, b'\n' | b'\r')) {
            let cr = bytes[end] == b'\r';
            line.extend_from_slice(&bytes[..end]);
            reader.consume(end + 1);
            if cr && reader.fill_buf()?.first() == Some(&b'\n') {
                reader.consume(1);
            }
            return Ok(true);
        }
        let length = bytes.len();
        line.extend_from_slice(bytes);
        reader.consume(length);
    }
}
fn select(line: &[u8], number: usize, keys: &[&str], values: &mut HashMap<String, Value>) {
    let mut boundary = line.len();
    let mut escaped = false;
    for (index, byte) in line.iter().copied().enumerate() {
        if !escaped && (space(byte) || matches!(byte, b'=' | b':')) {
            boundary = index;
            break;
        }
        escaped = !escaped && byte == b'\\';
    }
    let raw_key = &line[..boundary];
    let key = if raw_key.is_ascii() && !raw_key.contains(&b'\\') {
        Cow::Borrowed(std::str::from_utf8(raw_key).expect("ASCII key"))
    } else {
        let Ok(key) = decode(raw_key) else {
            return;
        };
        Cow::Owned(key)
    };
    if !keys.contains(&key.as_ref()) {
        return;
    }
    let mut value = boundary;
    while line.get(value).is_some_and(|byte| space(*byte)) {
        value += 1;
    }
    if line
        .get(value)
        .is_some_and(|byte| matches!(*byte, b'=' | b':'))
    {
        value += 1;
    }
    while line.get(value).is_some_and(|byte| space(*byte)) {
        value += 1;
    }
    values.insert(
        key.into_owned(),
        Value {
            bytes: line[value..].to_vec(),
            line: number,
        },
    );
}
fn unit(bytes: &[u8], position: &mut usize) -> Result<u16> {
    let digits = bytes
        .get(*position..*position + 4)
        .context("incomplete Unicode escape")?;
    let mut value = 0;
    for digit in digits {
        let digit = (*digit as char)
            .to_digit(16)
            .context("non-hexadecimal Unicode escape")?;
        value = (value << 4) | digit as u16;
    }
    *position += 4;
    Ok(value)
}
fn decode(bytes: &[u8]) -> Result<String> {
    let mut output = String::with_capacity(bytes.len());
    let mut position = 0;
    while let Some(&byte) = bytes.get(position) {
        position += 1;
        if byte != b'\\' {
            output.push(byte as char);
            continue;
        }
        let Some(&escape) = bytes.get(position) else {
            output.push('\0');
            break;
        };
        position += 1;
        match escape {
            b't' => output.push('\t'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b'f' => output.push('\u{c}'),
            b'u' => {
                let high = unit(bytes, &mut position)?;
                let code = if (0xd800..=0xdbff).contains(&high) {
                    if bytes.get(position..position + 2) != Some(b"\\u") {
                        bail!("unpaired Unicode surrogate");
                    }
                    position += 2;
                    let low = unit(bytes, &mut position)?;
                    anyhow::ensure!(
                        (0xdc00..=0xdfff).contains(&low),
                        "unpaired Unicode surrogate"
                    );
                    0x10000 + (((high as u32 - 0xd800) << 10) | (low as u32 - 0xdc00))
                } else {
                    high as u32
                };
                output.push(char::from_u32(code).context("unpaired Unicode surrogate")?);
            }
            other => output.push(other as char),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn read(text: &[u8]) -> Result<Properties> {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(text)?;
        Ok(
            Properties::read_selected(file.path(), &["enable-rcon", "rcon.port", "rcon.password"])?
                .unwrap(),
        )
    }
    #[test]
    fn unrelated_invalid_values_and_comments_do_not_validate() -> Result<()> {
        let properties = read(b"# invalid \\uZZZZ\\\r\nenable-rcon=true\r\nmotd=\\uBAD\rmax-players=garbage\nrcon.port=25575\nrcon.password=secret\n")?;
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("true"));
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25575"));
        assert_eq!(properties.get("rcon.password")?.as_deref(), Some("secret"));
        Ok(())
    }
    #[test]
    fn logical_lines_escaped_keys_duplicates_and_surrogates() -> Result<()> {
        let properties = read(b"rcon\\u002eport:1\nrcon.port  = 25575\nrcon.password=one\\\r\n  two\\\\three\\uD83D\\uDE00\nenable-rcon=true\n")?;
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25575"));
        assert_eq!(
            properties.get("rcon.password")?.as_deref(),
            Some("onetwo\\three😀")
        );
        Ok(())
    }
    #[test]
    fn unused_selected_values_are_not_decoded() -> Result<()> {
        let properties = read(b"enable-rcon=false\nrcon.port=\\uZZZZ\nrcon.password=\\uD800\n")?;
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("false"));
        assert!(properties.get("rcon.port").is_err());
        assert!(properties.get("rcon.password").is_err());
        Ok(())
    }
}
