use anyhow::Result;
use serde_json::Value;

pub(super) fn invalid(document: &str, field: &str, expected: &'static str) -> anyhow::Error {
    anyhow::anyhow!("{document}: invalid metadata field {field}; expected {expected}")
}

pub(super) fn required_string<'a>(value: &'a Value, key: &str, document: &str) -> Result<&'a str> {
    let field = value
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("{document}: missing metadata field {key}"))?;
    field
        .as_str()
        .ok_or_else(|| invalid(document, key, "a string"))
}

pub(super) fn optional_string<'a>(
    value: &'a Value,
    key: &str,
    document: &str,
) -> Result<Option<&'a str>> {
    value
        .get(key)
        .map(|field| {
            field
                .as_str()
                .ok_or_else(|| invalid(document, key, "a string"))
        })
        .transpose()
}

pub(super) fn optional_bool(value: &Value, key: &str, document: &str) -> Result<Option<bool>> {
    value
        .get(key)
        .map(|field| {
            field
                .as_bool()
                .ok_or_else(|| invalid(document, key, "a boolean"))
        })
        .transpose()
}

pub(super) fn object<'a>(
    value: &'a Value,
    document: &str,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid(document, field, "an object"))
}

pub(super) fn array<'a>(value: &'a Value, document: &str, field: &str) -> Result<&'a Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| invalid(document, field, "an array"))
}
