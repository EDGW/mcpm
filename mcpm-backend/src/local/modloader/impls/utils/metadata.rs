//! Shared metadata field access with distinct missing-field and invalid-value errors.

use serde_json::Value;
use thiserror::Error;

pub(in crate::local::modloader::impls) fn required_string<'a>(
    value: &'a Value,
    key: &str,
    document: &str,
) -> Result<&'a str, MetadataFieldError> {
    let field = required(value, key, document)?;
    field
        .as_str()
        .ok_or_else(|| invalid(document, key, "a string"))
}

pub(in crate::local::modloader::impls) fn optional_string<'a>(
    value: &'a Value,
    key: &str,
    document: &str,
) -> Result<Option<&'a str>, MetadataFieldError> {
    value
        .get(key)
        .map(|field| {
            field
                .as_str()
                .ok_or_else(|| invalid(document, key, "a string"))
        })
        .transpose()
}

pub(in crate::local::modloader::impls) fn optional_bool(
    value: &Value,
    key: &str,
    document: &str,
) -> Result<Option<bool>, MetadataFieldError> {
    value
        .get(key)
        .map(|field| {
            field
                .as_bool()
                .ok_or_else(|| invalid(document, key, "a boolean"))
        })
        .transpose()
}

pub(in crate::local::modloader::impls) fn object<'a>(
    value: &'a Value,
    document: &str,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, MetadataFieldError> {
    value
        .as_object()
        .ok_or_else(|| invalid(document, field, "an object"))
}

pub(in crate::local::modloader::impls) fn array<'a>(
    value: &'a Value,
    document: &str,
    field: &str,
) -> Result<&'a Vec<Value>, MetadataFieldError> {
    value
        .as_array()
        .ok_or_else(|| invalid(document, field, "an array"))
}

/// Reads a required field before a caller applies its format-specific interpretation.
pub(in crate::local::modloader::impls) fn required<'a>(
    value: &'a Value,
    key: &str,
    document: &str,
) -> Result<&'a Value, MetadataFieldError> {
    value
        .get(key)
        .ok_or_else(|| MetadataFieldError::MissingField {
            document: document.to_owned(),
            field: key.to_owned(),
        })
}

pub(in crate::local::modloader::impls) fn invalid(
    document: &str,
    field: &str,
    expected: &'static str,
) -> MetadataFieldError {
    MetadataFieldError::InvalidField {
        document: document.to_owned(),
        field: field.to_owned(),
        expected,
    }
}

/// Field errors shared by the built-in loaders; document and field identify the cause.
#[derive(Debug, Error)]
pub enum MetadataFieldError {
    /// A required metadata field is absent.
    #[error("{document}: missing metadata field {field}")]
    MissingField { document: String, field: String },
    /// A present field has the wrong type or fails a field constraint.
    #[error("{document}: invalid metadata field {field}; expected {expected}")]
    InvalidField {
        document: String,
        field: String,
        expected: &'static str,
    },
}
