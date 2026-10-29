//! Classifies parsed launch evidence and agent JAR metadata into fixed structure and user settings.

use super::*;
use crate::local::mc::server::ServerLaunch;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

/// Complete local initialization input; independent of a previous recognition session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationDraft {
    pub launch: LaunchConfig,
    pub imported: Vec<ImportedSetting>,
}

impl ConfigurationDraft {
    pub(crate) fn template(&self) -> Result<String> {
        template(&self.imported)
    }
}

/// An explicit source setting, enabled in a newly generated conf and returned to callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedSetting {
    pub field: String,
    pub value: ImportedValue,
}

/// Typed imported values rendered by the single configuration template owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImportedValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Arguments(Vec<String>),
}

impl ImportedValue {
    fn toml(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Value<'a> {
            value: &'a ImportedValue,
        }
        let text = toml::to_string(&Value { value: self })?;
        Ok(text
            .strip_prefix("value = ")
            .context("unexpected TOML value encoding")?
            .trim_end()
            .to_owned())
    }
}

/// Consumes parsed command evidence and agent metadata; never rereads scripts, writes files or runs Java.
pub fn draft(root: &Path, launch: &ServerLaunch) -> Result<ConfigurationDraft> {
    let root = std::fs::canonicalize(root)
        .context("failed to resolve server root for configuration draft")?;
    let directory = relative_directory(&root, &launch.directory)?;
    let executable = if executable_is_absolute(&launch.executable, launch.path_separator)
        || !launch.executable.contains(['/', '\\'])
    {
        launch.executable.clone()
    } else {
        relative_directory(&root, &launch.resolve(&launch.executable))?
    };
    ensure!(
        !launch.command.options().any(|(name, _)| matches!(
            managed(name).as_deref(),
            Some(
                "UnlockExperimentalVMOptions"
                    | "UnlockDiagnosticVMOptions"
                    | "IgnoreUnrecognizedVMOptions"
            )
        )) || !launch
            .command
            .options()
            .any(|(name, _)| name.starts_with("-XX:") && managed(name).is_none()),
        "cannot preserve unlock/ignore ordering with fixed JVM options; this combination is not supported by configuration import"
    );
    let mut imported = vec![ImportedSetting {
        field: "java.executable".into(),
        value: ImportedValue::String(executable),
    }];
    let (auth_settings, auth_consumed) = authlib_injector::import(root.as_path(), launch)?;
    imported.extend(auth_settings);
    let mut fixed = Vec::new();
    let mut memory = Vec::new();
    let mut index = 0;
    for (name, operand) in launch.command.options() {
        let raw = &launch.command.jvm[index];
        if let Some(path) = name.strip_prefix("-javaagent:") {
            let path = launch.resolve(path);
            let file = std::fs::File::open(&path)
                .with_context(|| format!("failed to open imported Java agent JAR {path:?}"))?;
            ensure!(
                file.metadata()
                    .with_context(|| format!("failed to inspect imported Java agent JAR {path:?}"))?
                    .is_file(),
                "imported Java agent JAR is not a file: {path:?}"
            );
        }
        let count = if raw.contains('=') || operand.is_none() {
            1
        } else {
            2
        };
        if auth_consumed.contains(&index) {
            // The structured authentication configuration owns this argument.
        } else if managed(raw).is_some() {
            memory.push(raw.clone());
        } else {
            fixed.extend_from_slice(&launch.command.jvm[index..index + count]);
        }
        index += count;
    }
    // Preserve complex combinations as one ordered region instead of silently changing precedence.
    let mut seen = HashSet::new();
    let mut fields = HashSet::new();
    let simple = memory.iter().all(|arg| {
        managed(arg).is_some_and(|key| seen.insert(key))
            && suggestion(arg).is_none_or(|(field, _)| fields.insert(field))
    }) && !memory.iter().any(|arg| {
        arg.starts_with("-XX:NewSize=")
            || arg.starts_with("-XX:MaxNewSize=")
            || arg.contains("Unlock")
            || arg.contains("IgnoreUnrecognizedVMOptions")
    });
    let mut remaining = Vec::new();
    for argument in &memory {
        let recognized = if simple { suggestion(argument) } else { None };
        if let Some((field, value)) = recognized {
            imported.push(ImportedSetting {
                field: field.into(),
                value,
            });
        } else {
            remaining.push(argument.clone());
        }
    }
    if !remaining.is_empty() {
        imported.push(ImportedSetting {
            field: "jvm.memory.extraArguments".into(),
            value: ImportedValue::Arguments(remaining),
        });
    }
    let mut application = launch.command.application.clone();
    if let Some(settings) = application::settings(&launch.command.application)? {
        let mut fields = HashSet::new();
        let extracted = settings
            .iter()
            .map(|setting| {
                if !fields.insert(setting.field) {
                    return None;
                }
                let value = match setting.field {
                    "server.nogui" => ImportedValue::Boolean(true),
                    "server.port" => {
                        ImportedValue::Integer(setting.value?.parse::<u16>().ok()?.into())
                    }
                    _ => ImportedValue::String(setting.value?.into()),
                };
                Some(ImportedSetting {
                    field: setting.field.into(),
                    value,
                })
            })
            .collect::<Option<Vec<_>>>();
        // Unrepresentable values and repeated options belong to the server. Preserve
        // their complete ordered region instead of validating or collapsing them.
        if let Some(extracted) = extracted {
            for setting in settings.iter().rev() {
                application.drain(setting.range.clone());
            }
            imported.extend(extracted);
        }
    }
    let draft = ConfigurationDraft {
        launch: LaunchConfig {
            version: 1,
            platform: Some(if launch.path_separator == ';' {
                Platform::Windows
            } else {
                Platform::Unix
            }),
            process: Process {
                working_directory: Some(directory),
                environment: launch
                    .environment
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            },
            jvm: Arguments {
                extra_arguments: fixed,
            },
            entry: launch.command.entry.clone(),
            application: Arguments {
                extra_arguments: application,
            },
        },
        imported,
    };
    draft.launch.validate()?;
    let conf = decode_conf(&draft.template()?)?;
    validate_pair(&draft.launch, &conf)?;
    Ok(draft)
}

pub(super) fn executable_is_absolute(path: &str, separator: char) -> bool {
    if separator == ';' {
        let bytes = path.as_bytes();
        path.starts_with(['\\', '/'])
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
    } else {
        Path::new(path).is_absolute()
    }
}

pub(super) fn relative_directory(root: &Path, directory: &Path) -> Result<String> {
    let mut base: Vec<_> = root.components().collect();
    let target: Vec<_> = directory.components().collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    base.drain(..common);
    let mut result = std::path::PathBuf::new();
    for _ in base {
        result.push("..");
    }
    for component in &target[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result
        .to_str()
        .context("working directory is not valid UTF-8")?
        .to_owned())
}

fn suggestion(argument: &str) -> Option<(&'static str, ImportedValue)> {
    for (prefix, field) in [
        ("-Xms", "jvm.heap.initial"),
        ("-Xmx", "jvm.heap.maximum"),
        ("-Xmn", "jvm.heap.young"),
        ("-Xss", "jvm.threadStack"),
        ("-XX:InitialHeapSize=", "jvm.heap.initial"),
        ("-XX:MaxHeapSize=", "jvm.heap.maximum"),
        ("-XX:MaxRAM=", "jvm.heap.maximumRAM"),
        ("-XX:MetaspaceSize=", "jvm.metaspace.initial"),
        ("-XX:MaxMetaspaceSize=", "jvm.metaspace.maximum"),
        (
            "-XX:CompressedClassSpaceSize=",
            "jvm.metaspace.compressedClassSpace",
        ),
        ("-XX:MaxDirectMemorySize=", "jvm.direct.maximum"),
        ("-XX:InitialCodeCacheSize=", "jvm.codeCache.initial"),
        ("-XX:ReservedCodeCacheSize=", "jvm.codeCache.maximum"),
    ] {
        if let Some(value) = argument.strip_prefix(prefix) {
            return Some((field, ImportedValue::String(value.into())));
        }
    }
    for (flag, collector) in [
        ("UseSerialGC", "serial"),
        ("UseParallelGC", "parallel"),
        ("UseG1GC", "g1"),
        ("UseZGC", "zgc"),
        ("UseShenandoahGC", "shenandoah"),
        ("UseEpsilonGC", "epsilon"),
    ] {
        if argument == format!("-XX:+{flag}") {
            return Some(("jvm.gc.collector", ImportedValue::String(collector.into())));
        }
    }
    None
}

pub(crate) fn default_template() -> Result<String> {
    template(&[])
}

fn template(suggestions: &[ImportedSetting]) -> Result<String> {
    let mut remaining = BTreeMap::new();
    for setting in suggestions {
        ensure!(
            remaining
                .insert(setting.field.as_str(), &setting.value)
                .is_none(),
            "repeated imported setting {} cannot be represented as one setting",
            setting.field
        );
    }
    let mut table = String::new();
    let mut result = String::new();
    let mut skip_array = false;
    for line in include_str!("conf.template.toml").lines() {
        if skip_array {
            if line.trim_start().starts_with("# ]") {
                skip_array = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("# [") {
            table = rest.split(']').next().unwrap_or("").into();
            if suggestions.iter().any(|setting| {
                setting
                    .field
                    .rsplit_once('.')
                    .is_some_and(|(parent, _)| parent == table)
            }) {
                result.push_str(&line[2..]);
                result.push('\n');
                continue;
            }
        } else if line.starts_with("[java]") {
            table = "java".into();
        }
        if let Some((key, rest)) = line
            .strip_prefix("# ")
            .and_then(|line| line.split_once(" = "))
        {
            let path = if table.is_empty() {
                key.to_owned()
            } else {
                format!("{table}.{key}")
            };
            if let Some(value) = remaining.remove(path.as_str()) {
                let description = rest
                    .split_once('#')
                    .map(|(_, description)| description.trim())
                    .unwrap_or("");
                let encoded = format!("{key} = {}", value.toml()?);
                for line in encoded.lines() {
                    result.push_str(line);
                    result.push('\n');
                }
                if !description.is_empty() {
                    result.push_str("# ");
                    result.push_str(description);
                    result.push('\n');
                }
                skip_array =
                    rest.trim_start().starts_with('[') && !rest.trim_start().starts_with("[]");
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    ensure!(
        remaining.is_empty(),
        "unsupported imported suggestion fields: {:?}",
        remaining.keys()
    );
    decode_conf(&result)?;
    Ok(result)
}
