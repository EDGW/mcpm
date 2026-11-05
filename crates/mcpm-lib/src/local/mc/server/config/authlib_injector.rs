//! Authentication configuration and lossless JVM property conversion using the config schema.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use toml::{Table, Value};

use super::{ImportedSetting, ImportedValue, ServerConfig, deserialize_conf, nonempty};
use crate::local::java::command::JavaCommand;

/// Explicit agent configuration; a missing or empty JAR path selects server-local management.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthlibInjector {
    pub jar: Option<String>,
    #[serde(default)]
    pub api_url: String,
    pub no_log_file: Option<bool>,
    pub mojang_namespace: Option<FeatureOption>,
    pub mojang_proxy: Option<String>,
    pub legacy_skin_polyfill: Option<FeatureOption>,
    #[serde(default)]
    pub debug: Vec<String>,
    #[serde(default)]
    pub ignored_packages: Vec<String>,
    pub disable_httpd: Option<bool>,
    pub httpd_port: Option<u16>,
    pub no_show_server_name: Option<bool>,
    pub mojang_anti_features: Option<FeatureOption>,
    pub profile_key: Option<FeatureOption>,
    pub username_check: Option<FeatureOption>,
}

impl AuthlibInjector {
    /// None means managed; paths are resolved by launch, not during TOML decoding.
    pub fn manual_jar(&self) -> Option<&str> {
        self.jar.as_deref().filter(|path| !path.is_empty())
    }

    pub(super) fn validate(&self) -> Result<()> {
        nonempty(&self.api_url, "authlib-injector.apiUrl")?;
        if let Some(path) = &self.jar {
            ensure!(
                !path.contains(['\0', '=']),
                "authlib-injector.jar contains NUL or the agent option separator '='"
            );
        }
        for (key, value) in self.fields()? {
            let values = match value {
                Value::Array(values) => values,
                value => vec![value],
            };
            for value in values {
                if let Value::String(value) = value {
                    ensure!(!value.contains('\0'), "NUL in authlib-injector.{key}");
                    if matches!(key.as_str(), "debug" | "ignoredPackages") {
                        ensure!(
                            !value.is_empty() && !value.contains(','),
                            "authlib-injector.{key} items must be nonempty and cannot contain commas"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Generates one argv item per property; agent paths must already be resolved.
    pub(crate) fn arguments(&self, jar: &Path) -> Result<Vec<String>> {
        let path = jar
            .to_str()
            .context("authlib-injector JAR path is not UTF-8")?;
        ensure!(
            !path.contains('='),
            "authlib-injector JAR path contains the agent option separator '='"
        );
        let mut result = vec![format!("-javaagent:{path}={}", self.api_url)];
        for (key, value) in self.fields()? {
            if matches!(key.as_str(), "jar" | "apiUrl") {
                continue;
            }
            let value = match value {
                Value::Boolean(true) => {
                    result.push(format!("-Dauthlibinjector.{key}"));
                    continue;
                }
                Value::Boolean(false) => continue,
                Value::String(value) => value,
                Value::Integer(value) => value.to_string(),
                Value::Array(values) if values.is_empty() => continue,
                Value::Array(values) => values
                    .iter()
                    .map(|value| value.as_str().expect("typed string array"))
                    .collect::<Vec<_>>()
                    .join(","),
                _ => unreachable!("authentication schema contains only properties"),
            };
            result.push(format!("-Dauthlibinjector.{key}={value}"));
        }
        Ok(result)
    }

    fn fields(&self) -> Result<Table> {
        Value::try_from(self)?
            .as_table()
            .cloned()
            .context("authentication schema must be a table")
    }

    pub(super) fn validate_properties(&self, arguments: &[String]) -> Result<()> {
        let fields = self.fields()?;
        let mut probe = arguments.to_vec();
        probe.push("mcpm.validation.Main".into());
        for (name, _) in JavaCommand::parse(&probe)?.options() {
            if let Some(key) = name.strip_prefix("-Dauthlibinjector.") {
                let owned = !matches!(key, "jar" | "apiUrl")
                    && fields.get(key).is_some_and(
                        |value| !matches!(value, Value::Array(values) if values.is_empty()),
                    );
                ensure!(
                    !owned,
                    "authlib-injector.{key} is configured both as a structured field and a raw JVM property"
                );
            }
        }
        Ok(())
    }

    /// Extracts representable properties, retaining repeated or unsupported values as raw argv.
    pub(super) fn import_properties(
        arguments: &[(usize, &str, Option<&str>)],
    ) -> Result<(Vec<ImportedSetting>, HashSet<usize>)> {
        let mut settings = Vec::new();
        let mut consumed = HashSet::new();
        for &(index, name, value) in arguments {
            let Some(key) = name.strip_prefix("-Dauthlibinjector.") else {
                continue;
            };
            if arguments
                .iter()
                .filter(|(_, other, _)| *other == name)
                .count()
                != 1
            {
                continue;
            }
            let Some(value) = property_value(key, value) else {
                continue;
            };
            // The configuration codec remains the sole authority on field names and types.
            let mut probe = Value::try_from(ServerConfig::default())?;
            probe.as_table_mut().expect("configuration table").insert(
                "authlib-injector".into(),
                Value::Table(Table::from_iter([(key.into(), value.clone())])),
            );
            if deserialize_conf(probe).is_err() {
                continue;
            }
            let value = match value {
                Value::String(value) => ImportedValue::String(value),
                Value::Boolean(value) => ImportedValue::Boolean(value),
                Value::Integer(value) => ImportedValue::Integer(value),
                Value::Array(values) => ImportedValue::Arguments(
                    values
                        .into_iter()
                        .map(|value| value.as_str().expect("string array").to_owned())
                        .collect(),
                ),
                _ => unreachable!("property conversion produces scalar or array"),
            };
            settings.push(ImportedSetting {
                field: format!("authlib-injector.{key}"),
                value,
            });
            consumed.insert(index);
        }
        Ok((settings, consumed))
    }
}

/// Classifies agent identity using JAR metadata, and extracts only its own JVM properties.
pub(super) fn import(
    root: &Path,
    launch: &crate::local::mc::server::ServerLaunch,
) -> Result<(Vec<ImportedSetting>, HashSet<usize>)> {
    let mut index = 0;
    let mut options = Vec::new();
    let mut agent = None;
    for (name, value) in launch.command.options() {
        let raw = &launch.command.jvm[index];
        options.push((index, name, value));
        if let Some((path, api_url)) = crate::local::mc::server::authlib_injector::agent(raw)
            && crate::local::mc::server::authlib_injector::is_agent(&launch.resolve(path))
        {
            ensure!(
                agent.is_none(),
                "multiple authlib-injector agents cannot be imported as one configuration"
            );
            let path = if super::import::executable_is_absolute(path, launch.path_separator) {
                path.to_owned()
            } else {
                super::import::relative_directory(root, &launch.resolve(path))?
            };
            agent = Some((index, path, api_url));
        }
        index += if raw.contains('=') || value.is_none() {
            1
        } else {
            2
        };
    }
    let Some((index, path, api_url)) = agent else {
        return Ok((Vec::new(), HashSet::new()));
    };
    let (mut settings, mut consumed) = AuthlibInjector::import_properties(&options)?;
    consumed.insert(index);
    settings.extend([
        ImportedSetting {
            field: "authlib-injector.jar".into(),
            value: ImportedValue::String(path),
        },
        ImportedSetting {
            field: "authlib-injector.apiUrl".into(),
            value: ImportedValue::String(api_url.into()),
        },
    ]);
    Ok((settings, consumed))
}

/// Feature behavior chosen by the agent, with explicit upstream default support.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureOption {
    Default,
    Enabled,
    Disabled,
}

fn property_value(key: &str, value: Option<&str>) -> Option<Value> {
    let value = value.unwrap_or("");
    Some(match key {
        "jar" | "apiUrl" => return None,
        "noLogFile" | "disableHttpd" | "noShowServerName" => Value::Boolean(true),
        "httpdPort" => Value::Integer(value.parse::<u16>().ok()?.into()),
        "debug" if value.is_empty() => Value::Array(vec![
            Value::String("verbose".into()),
            Value::String("authlib".into()),
        ]),
        "debug" | "ignoredPackages" => {
            let values = value
                .split(',')
                .map(|value| Value::String(value.into()))
                .collect::<Vec<_>>();
            if values.iter().any(|value| value.as_str() == Some("")) {
                return None;
            }
            Value::Array(values)
        }
        _ => Value::String(value.into()),
    })
}
