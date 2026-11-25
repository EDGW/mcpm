//! Authentication configuration and lossless JVM property conversion using the config schema.

use anyhow::{Context, Result, ensure};
use mcpm_macros::ConfigFields;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use toml::Value;

use super::{ImportedSetting, ImportedValue, nonempty, overrides::ConfigFields};
use crate::local::java::command;

/// Explicit agent configuration; a missing or empty JAR path selects server-local management.
#[derive(Debug, Default, Clone, Serialize, Deserialize, ConfigFields)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthlibInjector {
    pub jar: Option<String>,
    #[serde(default)]
    /// Authentication API URL for authlib-injector.
    #[config(command = "auth", command_short = 'A')]
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
        if let Some(proxy) = &self.mojang_proxy {
            ensure!(
                !proxy.contains('\0'),
                "NUL in authlib-injector.mojangProxy"
            );
        }
        for (key, values) in [
            ("debug", &self.debug),
            ("ignoredPackages", &self.ignored_packages),
        ] {
            for value in values {
                ensure!(!value.contains('\0'), "NUL in authlib-injector.{key}");
                ensure!(
                    !value.is_empty() && !value.contains(','),
                    "authlib-injector.{key} items must be nonempty and cannot contain commas"
                );
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
        for (key, value) in [
            ("noLogFile", self.no_log_file),
            ("disableHttpd", self.disable_httpd),
            ("noShowServerName", self.no_show_server_name),
        ] {
            if value == Some(true) {
                result.push(format!("-Dauthlibinjector.{key}"));
            }
        }
        for (key, value) in [
            ("mojangNamespace", self.mojang_namespace),
            ("legacySkinPolyfill", self.legacy_skin_polyfill),
            ("mojangAntiFeatures", self.mojang_anti_features),
            ("profileKey", self.profile_key),
            ("usernameCheck", self.username_check),
        ] {
            if let Some(value) = value {
                result.push(format!("-Dauthlibinjector.{key}={}", value.as_str()));
            }
        }
        if let Some(proxy) = &self.mojang_proxy {
            result.push(format!("-Dauthlibinjector.mojangProxy={proxy}"));
        }
        if let Some(port) = self.httpd_port {
            result.push(format!("-Dauthlibinjector.httpdPort={port}"));
        }
        for (key, values) in [
            ("debug", &self.debug),
            ("ignoredPackages", &self.ignored_packages),
        ] {
            if !values.is_empty() {
                result.push(format!("-Dauthlibinjector.{key}={}", values.join(",")));
            }
        }
        Ok(result)
    }

    pub(super) fn validate_properties(&self, arguments: &[String]) -> Result<()> {
        for (name, _) in command::jvm_options(arguments) {
            if let Some(key) = name.strip_prefix("-Dauthlibinjector.") {
                ensure!(
                    !self.owns_property(key),
                    "authlib-injector.{key} is configured both as a structured field and a raw JVM property"
                );
            }
        }
        Ok(())
    }

    fn owns_property(&self, key: &str) -> bool {
        match key {
            "noLogFile" => self.no_log_file.is_some(),
            "mojangNamespace" => self.mojang_namespace.is_some(),
            "mojangProxy" => self.mojang_proxy.is_some(),
            "legacySkinPolyfill" => self.legacy_skin_polyfill.is_some(),
            "debug" => !self.debug.is_empty(),
            "ignoredPackages" => !self.ignored_packages.is_empty(),
            "disableHttpd" => self.disable_httpd.is_some(),
            "httpdPort" => self.httpd_port.is_some(),
            "noShowServerName" => self.no_show_server_name.is_some(),
            "mojangAntiFeatures" => self.mojang_anti_features.is_some(),
            "profileKey" => self.profile_key.is_some(),
            "usernameCheck" => self.username_check.is_some(),
            _ => false,
        }
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
            if Self::field(key).is_none_or(|field| field.check_value(&value).is_err()) {
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

impl FeatureOption {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
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
