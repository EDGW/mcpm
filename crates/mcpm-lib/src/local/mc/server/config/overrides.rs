//! Typed, ordered changes to the launch-time `ServerConfig`, with the existing operation codec.

use super::{Collector, FeatureOption, ServerConfig};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::value::StrDeserializer};
use std::collections::HashSet;
use toml::Value;

/// Launch-only overrides. Scalars are unique; list operations preserve their input order.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct ConfigurationOverrides {
    operations: Vec<Operation>,
    #[serde(skip)]
    scalars: HashSet<String>,
}

impl ConfigurationOverrides {
    /// Parses key=value, key:=value, key+value or key+=value using the target field's type.
    pub fn parse(&mut self, assignment: &str) -> Result<()> {
        self.parse_assignment(assignment, false)
    }

    /// Parses the same operations as [`Self::parse`], also accepting a unique shortcut name.
    pub fn parse_with_aliases(&mut self, assignment: &str) -> Result<()> {
        self.parse_assignment(assignment, true)
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    fn parse_assignment(&mut self, assignment: &str, aliases: bool) -> Result<()> {
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
        if aliases && ServerConfig::field(key).is_none() {
            let key = shortcut_path(key)?;
            return self.parse_field(&key, raw, mode, false);
        }
        self.parse_field(key, raw, mode, false)
    }

    /// Adds a single CLI operand without interpreting quotes or array syntax in string values.
    pub fn set_shortcut(&mut self, key: &str, value: &str) -> Result<()> {
        self.parse_field(key, value, Mode::Assign, true)
    }

    /// Adds a typed convenience option through the same field and duplicate checks.
    pub fn set(&mut self, key: &str, value: impl Serialize) -> Result<()> {
        self.push(key, Value::try_from(value)?, Mode::Assign, field(key)?)
    }

    /// The candidate aliases; each frontend filters them against its own command tree.
    pub fn shortcuts() -> Vec<ConfigurationShortcut> {
        let mut fields = Vec::new();
        ServerConfig::visit_shortcuts(&mut Vec::new(), &mut |path, field| {
            fields.push(ConfigurationShortcut {
                path: path.join("."),
                command: field.command,
                command_short: field.command_short,
                help: field.help,
                list: field.list,
            });
        });
        fields
    }

    fn parse_field(&mut self, key: &str, raw: &str, mode: Mode, shortcut: bool) -> Result<()> {
        let field = field(key)?;
        let value = if field.list && raw.is_empty() && matches!(mode, Mode::ReplaceList) {
            Value::Array(Vec::new())
        } else {
            (field.parse)(raw, shortcut).with_context(|| format!("invalid override {key}"))?
        };
        self.push(key, value, mode, field)
    }

    fn push(&mut self, key: &str, value: Value, mode: Mode, field: Field) -> Result<()> {
        ensure!(
            field.list || matches!(mode, Mode::Assign),
            "list operator requires a list field: {key}"
        );
        let value = if field.list && !value.is_array() {
            Value::Array(vec![value])
        } else {
            value
        };
        field
            .check_value(&value)
            .with_context(|| format!("invalid override {key}"))?;
        ensure!(
            field.list || self.scalars.insert(key.into()),
            "configuration key overridden more than once: {key}"
        );
        self.operations.push(Operation {
            key: key.into(),
            value,
            mode,
        });
        Ok(())
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &str> {
        self.operations
            .iter()
            .map(|operation| operation.key.as_str())
    }

    pub(super) fn apply(&self, config: &mut ServerConfig) -> Result<()> {
        for Operation { key, value, mode } in &self.operations {
            config
                .apply_field(key, value, *mode)
                .with_context(|| format!("invalid override {key}"))?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ConfigurationOverrides {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let operations = Vec::<Operation>::deserialize(deserializer)?;
        let mut overrides = Self::default();
        for Operation { key, value, mode } in operations {
            let field = field(&key).map_err(serde::de::Error::custom)?;
            overrides
                .push(&key, value, mode, field)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(overrides)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Operation {
    key: String,
    value: Value,
    mode: Mode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) enum Mode {
    Assign,
    ReplaceList,
    AppendList,
}

/// A candidate launch shortcut declared next to its configuration field.
pub struct ConfigurationShortcut {
    pub path: String,
    pub command: Option<&'static str>,
    pub command_short: Option<char>,
    pub help: &'static str,
    pub list: bool,
}

fn field(key: &str) -> Result<Field> {
    ServerConfig::field(key).with_context(|| format!("unknown configuration field: {key}"))
}

fn shortcut_path(name: &str) -> Result<String> {
    let short = name.chars().next().filter(|ch| ch.len_utf8() == name.len());
    let mut matches = Vec::new();
    ServerConfig::visit_shortcuts(&mut Vec::new(), &mut |path, field| {
        if field.command == Some(name)
            || short.is_some_and(|name| field.command_short == Some(name))
        {
            matches.push(path.join("."));
        }
    });
    match matches.len() {
        0 => anyhow::bail!("unknown configuration field: {name}"),
        1 => Ok(matches.pop().unwrap()),
        _ => anyhow::bail!(
            "ambiguous configuration shortcut {name:?}; use one of: {}",
            matches.join(", ")
        ),
    }
}

/// Field lookup, enumeration and assignment emitted by the configuration derive.
pub(super) trait ConfigFields {
    fn field(path: &str) -> Option<Field>;
    fn apply_field(&mut self, path: &str, value: &Value, mode: Mode) -> Result<()>;
    fn visit_shortcuts(
        prefix: &mut Vec<&'static str>,
        visitor: &mut dyn FnMut(&[&'static str], Field),
    );
}

pub(super) struct Field {
    command: Option<&'static str>,
    command_short: Option<char>,
    help: &'static str,
    list: bool,
    parse: fn(&str, bool) -> Result<Value>,
    check: fn(&Value) -> Result<()>,
}

impl Field {
    pub(super) fn of<T: FieldValue>(
        command: Option<&'static str>,
        command_short: Option<char>,
        help: &'static str,
    ) -> Self {
        Self {
            command,
            command_short,
            help,
            list: T::LIST,
            parse: T::parse,
            check: T::check,
        }
    }

    pub(super) fn check_value(&self, value: &Value) -> Result<()> {
        (self.check)(value)
    }
}

pub(super) trait FieldValue {
    const LIST: bool = false;
    fn parse(raw: &str, shortcut: bool) -> Result<Value>;
    fn check(value: &Value) -> Result<()>;
    fn apply(&mut self, value: &Value, mode: Mode) -> Result<()>;
}

impl<T: Scalar> FieldValue for Option<T> {
    fn parse(raw: &str, shortcut: bool) -> Result<Value> {
        T::parse(raw, shortcut)
    }

    fn check(value: &Value) -> Result<()> {
        T::check(value)
    }

    fn apply(&mut self, value: &Value, _: Mode) -> Result<()> {
        *self = Some(T::decode(value)?);
        Ok(())
    }
}

impl<T: Scalar> FieldValue for Vec<T> {
    const LIST: bool = true;

    fn parse(raw: &str, shortcut: bool) -> Result<Value> {
        if !shortcut && raw.trim_start().starts_with('[') {
            let value = raw.parse::<Value>()?;
            ensure!(value.is_array(), "expected a TOML array");
            return Ok(value);
        }
        T::parse(raw, shortcut)
    }

    fn check(value: &Value) -> Result<()> {
        value
            .as_array()
            .context("expected an array")?
            .iter()
            .try_for_each(T::check)
    }

    fn apply(&mut self, value: &Value, mode: Mode) -> Result<()> {
        let values = value.as_array().context("expected an array")?;
        if matches!(mode, Mode::ReplaceList) {
            self.clear();
        }
        self.reserve(values.len());
        for value in values {
            self.push(T::decode(value)?);
        }
        Ok(())
    }
}

trait Scalar: Sized {
    fn parse(raw: &str, _shortcut: bool) -> Result<Value> {
        Ok(raw.parse()?)
    }

    fn check(value: &Value) -> Result<()> {
        Self::decode(value).map(|_| ())
    }

    fn decode(value: &Value) -> Result<Self>;
}

macro_rules! direct_value {
    ($($ty:ty),+ $(,)?) => {$(
        impl FieldValue for $ty {
            fn parse(raw: &str, shortcut: bool) -> Result<Value> {
                <Self as Scalar>::parse(raw, shortcut)
            }
            fn check(value: &Value) -> Result<()> {
                <Self as Scalar>::check(value)
            }
            fn apply(&mut self, value: &Value, _: Mode) -> Result<()> {
                *self = Self::decode(value)?;
                Ok(())
            }
        }
    )+};
}

direct_value!(String, u32);

impl Scalar for String {
    fn parse(raw: &str, shortcut: bool) -> Result<Value> {
        if !shortcut
            && raw.trim_start().starts_with(['"', '\''])
            && let Ok(value @ Value::String(_)) = raw.parse()
        {
            return Ok(value);
        }
        Ok(Value::String(raw.into()))
    }

    fn check(value: &Value) -> Result<()> {
        ensure!(value.is_str(), "expected a string");
        Ok(())
    }

    fn decode(value: &Value) -> Result<Self> {
        Ok(value.as_str().context("expected a string")?.to_owned())
    }
}

impl Scalar for bool {
    fn decode(value: &Value) -> Result<Self> {
        value.as_bool().context("expected a boolean")
    }
}

macro_rules! unsigned_value {
    ($($ty:ty),+ $(,)?) => {$(
        impl Scalar for $ty {
            fn decode(value: &Value) -> Result<Self> {
                let integer = value.as_integer().context("expected an integer")?;
                Self::try_from(integer).with_context(|| format!("integer is out of range for {}", stringify!($ty)))
            }
        }
    )+};
}

unsigned_value!(u16, u32);

impl Scalar for f64 {
    fn decode(value: &Value) -> Result<Self> {
        value
            .as_float()
            .or_else(|| value.as_integer().map(|integer| integer as f64))
            .context("expected a number")
    }
}

macro_rules! named_value {
    ($($ty:ty),+ $(,)?) => {$(
        impl Scalar for $ty {
            fn parse(raw: &str, shortcut: bool) -> Result<Value> {
                <String as Scalar>::parse(raw, shortcut)
            }

            fn decode(value: &Value) -> Result<Self> {
                let name = value.as_str().context("expected a string name")?;
                Ok(Self::deserialize(StrDeserializer::<serde::de::value::Error>::new(name))?)
            }
        }
    )+};
}

named_value!(Collector, FeatureOption);
