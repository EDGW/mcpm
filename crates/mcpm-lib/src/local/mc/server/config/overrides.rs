//! Ordered runtime changes to conf fields; field names and types come from the existing schema.

use super::{ServerConfig, deserialize_conf};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::HashSet;
use toml::{Table, Value};

/// Launch-only overrides. Scalars are unique; list operations preserve their input order.
#[derive(Debug, Default)]
pub struct ConfigurationOverrides {
    operations: Vec<Operation>,
    scalars: HashSet<String>,
}

impl ConfigurationOverrides {
    /// Parses key=value, key:=value, key+value or key+=value.
    /// Values accept TOML literals or unquoted strings; list values may be single items.
    pub fn parse(&mut self, assignment: &str) -> Result<()> {
        let boundary = assignment
            .find(['=', '+', ':'])
            .context("override requires key=value, key:=value or key+value")?;
        let (key, rest) = assignment.split_at(boundary);
        let (mode, raw) = if let Some(raw) = rest.strip_prefix(":=") {
            (Mode::ReplaceList, raw)
        } else if let Some(raw) = rest.strip_prefix('+') {
            (Mode::AppendList, raw.strip_prefix('=').unwrap_or(raw))
        } else {
            (
                Mode::Assign,
                rest.strip_prefix('=')
                    .context("expected := after override key")?,
            )
        };
        // Parse only a value: an override must never inject an extra TOML assignment.
        let value = raw
            .parse::<Value>()
            .unwrap_or_else(|_| Value::String(raw.into()));
        self.push(key, value, mode, Some(raw))
    }

    /// Adds a typed convenience option through the same key/type/duplicate checks.
    pub fn set(&mut self, key: &str, value: impl Serialize) -> Result<()> {
        self.push(key, Value::try_from(value)?, Mode::Assign, None)
    }

    fn push(&mut self, key: &str, mut value: Value, mode: Mode, raw: Option<&str>) -> Result<()> {
        ensure!(
            key.split('.').all(|part| !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))),
            "invalid override key {key:?}"
        );
        // Include optional tables only in the type probe, never in effective defaults.
        let schema = Value::try_from(ServerConfig {
            authlib_injector: Some(super::AuthlibInjector::default()),
            ..ServerConfig::default()
        })?;
        let list = lookup(&schema, key).is_some_and(Value::is_array);
        ensure!(
            !lookup(&schema, key).is_some_and(Value::is_table),
            "override requires a field, not a table: {key}"
        );
        ensure!(
            list || matches!(mode, Mode::Assign),
            "list operator requires a list field: {key}"
        );
        if list {
            value = match value {
                Value::Array(_) => value,
                _ if raw == Some("") && matches!(mode, Mode::ReplaceList) => {
                    Value::Array(Vec::new())
                }
                item => Value::Array(vec![item]),
            };
        }
        let mut probe = schema.clone();
        insert(&mut probe, key, value.clone())?;
        if deserialize_conf(probe).is_err()
            && !list
            && !value.is_str()
            && let Some(raw) = raw
        {
            // Numeric/boolean-looking strings remain usable for string-typed fields.
            value = Value::String(raw.into());
        }
        let mut probe = schema;
        insert(&mut probe, key, value.clone())?;
        deserialize_conf(probe).with_context(|| format!("invalid override {key}"))?;
        ensure!(
            list || self.scalars.insert(key.into()),
            "configuration key overridden more than once: {key}"
        );
        self.operations.push(Operation {
            key: key.into(),
            value,
            mode,
        });
        Ok(())
    }

    pub(super) fn apply(&self, config: &mut Value) -> Result<()> {
        for operation in &self.operations {
            let value = match &operation.value {
                Value::Array(items) if !matches!(operation.mode, Mode::ReplaceList) => {
                    let mut values = lookup(config, &operation.key)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    values.extend(items.iter().cloned());
                    Value::Array(values)
                }
                value => value.clone(),
            };
            insert(config, &operation.key, value)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Operation {
    key: String,
    value: Value,
    mode: Mode,
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Assign,
    ReplaceList,
    AppendList,
}

fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(root, |value, part| value.get(part))
}

fn insert(root: &mut Value, key: &str, value: Value) -> Result<()> {
    let mut parts = key.split('.').peekable();
    let mut current = root;
    while let Some(part) = parts.next() {
        let table = current
            .as_table_mut()
            .context("override traverses a non-table field")?;
        if parts.peek().is_none() {
            table.insert(part.into(), value);
            return Ok(());
        }
        current = table
            .entry(part)
            .or_insert_with(|| Value::Table(Table::new()));
    }
    unreachable!("validated override key is nonempty")
}
