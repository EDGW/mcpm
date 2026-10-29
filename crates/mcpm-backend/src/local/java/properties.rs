//! Parsing of the conservative Java Properties subset supported by local inspection.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};

/// Parses a conservative subset of java.util.Properties; exotic escapes are rejected.
pub(crate) fn parse(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    for (index, line) in std::str::from_utf8(bytes)?.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', '!']) {
            continue;
        }
        ensure!(
            !line.contains('\\'),
            "escaped or continued Java properties are unsupported at line {}",
            index + 1
        );
        let index = line
            .find(['=', ':'])
            .or_else(|| line.find(char::is_whitespace))
            .with_context(|| format!("invalid Java property at line {}", index + 1))?;
        let key = line[..index].trim();
        let value = line[index..]
            .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=' || ch == ':')
            .trim_end();
        ensure!(
            !key.is_empty() && values.insert(key.to_owned(), value.to_owned()).is_none(),
            "duplicate or empty property {key:?} at line {}",
            index + 1
        );
    }
    Ok(values)
}
