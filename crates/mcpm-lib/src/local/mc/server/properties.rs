//! Java Properties codec: selective/full decoding, lossless key upsert, and no-clobber RCON creation.

use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Cursor, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};

/// Minecraft's conventional RCON port.
pub const DEFAULT_RCON_PORT: u16 = 25575;

/// Selected raw values are decoded lazily, so disabled features do not validate unused settings.
pub struct Properties {
    values: HashMap<String, Value>,
}

enum Value {
    Raw { bytes: Vec<u8>, line: usize },
    Decoded(String),
}

impl Properties {
    pub fn read_selected(path: &Path, keys: &[&str]) -> Result<Option<Self>> {
        Self::read_file(path, Some(keys))
    }

    /// Parses every key and value, allowing callers to distinguish a malformed document
    /// from invalid typed settings. Selected reads retain their existing lazy semantics.
    pub fn read(path: &Path) -> Result<Option<Self>> {
        Self::read_file(path, None)
    }

    fn read_file(path: &Path, keys: Option<&[&str]>) -> Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read properties {path:?}"));
            }
        };
        let mut reader = BufReader::new(file);
        read_reader(&mut reader, keys).map(Some)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.values
            .get(key)
            .map(|value| match value {
                Value::Decoded(value) => Ok(value.clone()),
                Value::Raw { bytes, line } => decode(bytes)
                    .with_context(|| format!("invalid properties value for {key} at line {line}")),
            })
            .transpose()
    }
}

/// Decodes all Java Properties entries from an ISO-8859-1 byte slice (last declaration wins).
pub fn parse_bytes(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut reader = Cursor::new(bytes);
    let properties = read_reader(&mut reader, None)?;
    Ok(properties
        .values
        .into_iter()
        .map(|(key, value)| match value {
            Value::Decoded(decoded) => (key, decoded),
            Value::Raw { .. } => unreachable!("full parse eagerly decodes every value"),
        })
        .collect())
}

/// Inserts or updates `key` in a Java Properties document while preserving comments,
/// blank lines, unrelated logical lines, and original line endings byte-for-byte.
///
/// If `key` appears multiple times, earlier declarations are dropped and the last
/// occurrence is replaced so Java's last-wins semantics remain unambiguous.
pub fn upsert_key(existing: Option<&[u8]>, key: &str, value: &str) -> Result<Vec<u8>> {
    let Some(bytes) = existing else {
        let mut output = Vec::new();
        encode_entry(&mut output, key, value, b"\n");
        return Ok(output);
    };

    let default_eol = detect_eol(bytes);
    let mut matches: Vec<(usize, usize, &[u8])> = Vec::new();
    let mut logical = Vec::new();
    let mut continuing = false;
    let mut span_start = 0;
    let mut cursor = 0;

    while cursor < bytes.len() {
        let line_start = cursor;
        while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
            cursor += 1;
        }
        let content_end = cursor;
        if bytes.get(cursor) == Some(&b'\r') {
            cursor += 1;
            if bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
        } else if bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        let line_end = cursor;
        let line_eol = &bytes[content_end..line_end];

        let content = trim_leading_space(&bytes[line_start..content_end]);
        if !continuing {
            if content.is_empty() || matches!(content[0], b'#' | b'!') {
                continue;
            }
            logical.clear();
            span_start = line_start;
        }
        logical.extend_from_slice(content);
        continuing = ends_with_continuation(content);
        if continuing {
            logical.pop();
            continue;
        }
        if logical_key_matches(&logical, key) {
            matches.push((span_start, line_end, line_eol));
        }
    }
    if continuing && logical_key_matches(&logical, key) {
        matches.push((span_start, bytes.len(), b""));
    }

    let mut output = Vec::with_capacity(bytes.len() + key.len() + value.len() + 4);
    if let Some((&(last_start, last_end, last_eol), earlier)) = matches.split_last() {
        let mut emitted = 0;
        for &(start, end, _) in earlier {
            output.extend_from_slice(&bytes[emitted..start]);
            emitted = end;
        }
        output.extend_from_slice(&bytes[emitted..last_start]);
        let eol = if !last_eol.is_empty() {
            last_eol
        } else {
            default_eol
        };
        encode_entry(&mut output, key, value, eol);
        output.extend_from_slice(&bytes[last_end..]);
    } else {
        output.extend_from_slice(bytes);
        if !bytes.is_empty() && !matches!(bytes.last(), Some(b'\n' | b'\r')) {
            output.extend_from_slice(default_eol);
        }
        if continuing {
            if bytes.ends_with(b"\r") && default_eol == b"\n" {
                output.push(b'\r');
            } else {
                output.extend_from_slice(default_eol);
            }
        }
        encode_entry(&mut output, key, value, default_eol);
    }

    Ok(output)
}

/// Creates only the three RCON properties without replacing an existing configuration.
/// Passwords are encoded with Java Properties escaping, including Unicode and line breaks.
pub fn create_rcon(path: &Path, port: u16, password: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create RCON properties {path:?}"))?;
    let mut content = Vec::new();
    let _ = writeln!(content, "enable-rcon=true\nrcon.port={port}");
    encode_entry(&mut content, "rcon.password", password, b"\n");
    file.write_all(&content)
        .with_context(|| format!("failed to write RCON properties {path:?}"))
}

fn read_reader(reader: &mut impl BufRead, keys: Option<&[&str]>) -> Result<Properties> {
    let mut physical = Vec::new();
    let mut logical = Vec::new();
    let mut values = HashMap::new();
    let mut continuing = false;
    let mut line = 0;
    let mut start = 0;
    while physical_line(reader, &mut physical)? {
        line += 1;
        let content = trim_leading_space(&physical);
        if !continuing {
            if content.is_empty() || matches!(content[0], b'#' | b'!') {
                continue;
            }
            logical.clear();
            start = line;
        }
        logical.extend_from_slice(content);
        continuing = ends_with_continuation(content);
        if continuing {
            logical.pop();
            continue;
        }
        select(&logical, start, keys, &mut values)?;
    }
    if continuing {
        select(&logical, start, keys, &mut values)?;
    }
    Ok(Properties { values })
}

fn encode_entry(out: &mut Vec<u8>, key: &str, value: &str, eol: &[u8]) {
    encode_key(out, key);
    out.push(b'=');
    encode_value(out, value);
    out.extend_from_slice(eol);
}

fn encode_key(out: &mut Vec<u8>, key: &str) {
    encode_component(out, key, true);
}

fn encode_value(out: &mut Vec<u8>, value: &str) {
    encode_component(out, value, false);
}

fn encode_component(out: &mut Vec<u8>, text: &str, is_key: bool) {
    let mut leading_space = true;
    for ch in text.chars() {
        if ch != ' ' {
            leading_space = false;
        }
        match ch {
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            ' ' if is_key || leading_space => out.extend_from_slice(b"\\ "),
            '=' | ':' | '#' | '!' if is_key => {
                out.push(b'\\');
                out.push(ch as u8);
            }
            ch if ch.is_ascii() && !ch.is_control() => out.push(ch as u8),
            ch => {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
        }
    }
}

fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0c)
}

fn trim_leading_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !space(*byte))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn ends_with_continuation(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn detect_eol(bytes: &[u8]) -> &'static [u8] {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\r' {
            return if bytes.get(cursor + 1) == Some(&b'\n') {
                b"\r\n"
            } else {
                b"\n"
            };
        }
        if bytes[cursor] == b'\n' {
            return b"\n";
        }
        cursor += 1;
    }
    b"\n"
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

fn key_boundary(line: &[u8]) -> usize {
    let mut escaped = false;
    for (index, byte) in line.iter().copied().enumerate() {
        if !escaped && (space(byte) || matches!(byte, b'=' | b':')) {
            return index;
        }
        escaped = !escaped && byte == b'\\';
    }
    line.len()
}

fn decode_key(raw_key: &[u8]) -> Result<Cow<'_, str>> {
    if raw_key.is_ascii() && !raw_key.contains(&b'\\') {
        Ok(Cow::Borrowed(
            std::str::from_utf8(raw_key).expect("ASCII key"),
        ))
    } else {
        decode(raw_key).map(Cow::Owned)
    }
}

fn logical_key_matches(logical: &[u8], target: &str) -> bool {
    let raw_key = &logical[..key_boundary(logical)];
    decode_key(raw_key).is_ok_and(|decoded| decoded.as_ref() == target)
}

fn select(
    line: &[u8],
    number: usize,
    keys: Option<&[&str]>,
    values: &mut HashMap<String, Value>,
) -> Result<()> {
    let boundary = key_boundary(line);
    let raw_key = &line[..boundary];
    let key = match decode_key(raw_key) {
        Ok(key) => key,
        Err(_) if keys.is_some() => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("invalid Java property at line {number}"));
        }
    };
    if keys.is_some_and(|keys| !keys.contains(&key.as_ref())) {
        return Ok(());
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
    let value = if keys.is_none() {
        Value::Decoded(
            decode(&line[value..])
                .with_context(|| format!("invalid Java property at line {number}"))?,
        )
    } else {
        Value::Raw {
            bytes: line[value..].to_vec(),
            line: number,
        }
    };
    values.insert(key.into_owned(), value);
    Ok(())
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

    #[test]
    fn rcon_creation_roundtrips_passwords_without_property_injection_or_overwrite() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("server.properties");
        let password = "  pass\\word=:#\nother=value\r\t世界😀";
        create_rcon(&path, 25590, password)?;
        let properties = Properties::read_selected(
            &path,
            &["enable-rcon", "rcon.port", "rcon.password", "other"],
        )?
        .unwrap();
        assert_eq!(properties.get("enable-rcon")?.as_deref(), Some("true"));
        assert_eq!(properties.get("rcon.port")?.as_deref(), Some("25590"));
        assert_eq!(properties.get("rcon.password")?.as_deref(), Some(password));
        assert_eq!(properties.get("other")?, None);
        let original = std::fs::read(&path)?;
        assert!(create_rcon(&path, 1, "replacement").is_err());
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }

    #[test]
    fn parse_bytes_matches_java_properties_edge_cases() -> Result<()> {
        let values = parse_bytes(
            b"a=old\na:new\nempty\n=x\nspace\\ key : =value  \ncontinued=hello\\\n  #part\\\r\n world\nescaped\\u003dkey=\\uD83D\\uDE00\ntrailing=abc\\",
        )?;
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
        assert!(parse_bytes(br"broken=\u12xz").is_err());
        Ok(())
    }

    #[test]
    fn upsert_key_preserves_document_collapses_duplicates_and_roundtrips_escapes() -> Result<()> {
        assert_eq!(
            upsert_key(None, "enable-rcon", "true")?,
            b"enable-rcon=true\n"
        );

        let crlf_doc = b"# header\r\n\r\nrcon\\u002eport : 1\r\nmotd = Keep # verbatim\r\nrcon.port=2\\\r\n  5575\r\nmax-players=20";
        let updated = upsert_key(Some(crlf_doc), "rcon.port", "25590")?;
        assert_eq!(
            updated,
            b"# header\r\n\r\nmotd = Keep # verbatim\r\nrcon.port=25590\r\nmax-players=20"
        );

        let appended = upsert_key(Some(&updated), "online-mode", "false")?;
        assert_eq!(
            appended,
            b"# header\r\n\r\nmotd = Keep # verbatim\r\nrcon.port=25590\r\nmax-players=20\r\nonline-mode=false\r\n"
        );

        let trailing_cont = upsert_key(Some(b"other=abc\\"), "added", "1")?;
        let parsed_cont = parse_bytes(&trailing_cont)?;
        assert_eq!(parsed_cont.get("other").map(String::as_str), Some("abc"));
        assert_eq!(parsed_cont.get("added").map(String::as_str), Some("1"));

        let trailing_cr_cont = upsert_key(Some(b"other=abc\\\r"), "added", "1")?;
        let parsed_cr_cont = parse_bytes(&trailing_cr_cont)?;
        assert_eq!(parsed_cr_cont.get("other").map(String::as_str), Some("abc"));
        assert_eq!(parsed_cr_cont.get("added").map(String::as_str), Some("1"));

        let key = " #!k=e:y\\ \t\n\r\u{c}😀";
        let value = "  v a\\l=:#!\n\r\t\u{c}世界😀  ";
        let encoded = upsert_key(None, key, value)?;
        let parsed = parse_bytes(&encoded)?;
        assert_eq!(parsed.get(key).map(String::as_str), Some(value));
        Ok(())
    }
}
