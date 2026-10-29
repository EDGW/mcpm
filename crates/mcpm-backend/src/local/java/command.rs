//! One Java launcher grammar for static imports, configuration validation and argument assembly.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// The launcher entry, without derived manifest or module-descriptor metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum JavaEntry {
    Class {
        #[serde(rename = "mainClass")]
        main_class: String,
    },
    Jar {
        path: String,
    },
    Module {
        module: String,
        #[serde(rename = "mainClass", skip_serializing_if = "Option::is_none")]
        main_class: Option<String>,
    },
}

impl JavaEntry {
    pub fn validate(&self) -> Result<()> {
        let no_nul = |value: &str| -> Result<()> {
            ensure!(!value.contains('\0'), "NUL in Java entry");
            Ok(())
        };
        match self {
            Self::Class { main_class } => {
                no_nul(main_class)?;
                ensure!(
                    !main_class.ends_with(".java"),
                    "Java source entries are unsupported"
                );
                ensure!(
                    !main_class.starts_with('-'),
                    "class entry changes launcher boundary"
                );
            }
            Self::Jar { path } => no_nul(path)?,
            Self::Module { module, main_class } => {
                no_nul(module)?;
                ensure!(
                    !module.contains('/'),
                    "module entry contains a main-class separator"
                );
                if let Some(main_class) = main_class {
                    no_nul(main_class)?;
                }
            }
        }
        Ok(())
    }

    pub fn arguments(&self) -> Vec<String> {
        match self {
            Self::Class { main_class } => vec![main_class.clone()],
            Self::Jar { path } => vec!["-jar".into(), path.clone()],
            Self::Module { module, main_class } => vec![
                "--module".into(),
                match main_class {
                    Some(main) => format!("{module}/{main}"),
                    None => module.clone(),
                },
            ],
        }
    }
}

/// Ordered argument regions. No shell text or live @file references are retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaCommand {
    pub jvm: Vec<String>,
    pub entry: JavaEntry,
    pub application: Vec<String>,
}

impl JavaCommand {
    /// Parses flattened arguments. Unknown option arity is an error, never a guessed entry.
    pub fn parse(arguments: &[String]) -> Result<Self> {
        let mut index = 0;
        while let Some(arg) = arguments.get(index) {
            ensure!(!arg.contains('\0'), "NUL in Java argument {index}");
            let entry = if arg == "-jar" {
                index += 1;
                Some(JavaEntry::Jar {
                    path: arguments.get(index).context("-jar has no path")?.clone(),
                })
            } else if matches!(arg.as_str(), "-m" | "--module") || arg.starts_with("--module=") {
                let value = if let Some(value) = arg.strip_prefix("--module=") {
                    value
                } else {
                    index += 1;
                    arguments
                        .get(index)
                        .context("module entry has no module name")?
                };
                let (module, main_class) = value
                    .split_once('/')
                    .map_or((value, None), |(module, main)| {
                        (module, Some(main.to_owned()))
                    });
                Some(JavaEntry::Module {
                    module: module.into(),
                    main_class,
                })
            } else if !arg.starts_with('-') {
                Some(JavaEntry::Class {
                    main_class: arg.clone(),
                })
            } else {
                match option(arg)? {
                    OptionArity::Operand => {
                        index += 1;
                        let operand = arguments
                            .get(index)
                            .with_context(|| format!("missing JVM option operand for {arg}"))?;
                        ensure!(!operand.contains('\0'), "NUL in JVM operand for {arg}");
                    }
                    OptionArity::Flag => {}
                }
                None
            };
            if let Some(entry) = entry {
                entry.validate()?;
                let entry_len = match &entry {
                    JavaEntry::Class { .. } => 1,
                    JavaEntry::Module { .. } if arg.starts_with("--module=") => 1,
                    _ => 2,
                };
                let application = arguments[index + 1..].to_vec();
                ensure!(
                    application.iter().all(|arg| !arg.contains('\0')),
                    "NUL in Java application argument"
                );
                return Ok(Self {
                    jvm: arguments[..index + 1 - entry_len].to_vec(),
                    entry,
                    application,
                });
            }
            index += 1;
        }
        bail!("Java command has no server entry point")
    }

    /// JVM options paired with their operands; path values are never reinterpreted as flags.
    pub(crate) fn options(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        let mut values = self.jvm.iter();
        std::iter::from_fn(move || {
            let arg = values.next()?;
            if let Some((name, value)) = arg.split_once('=') {
                Some((name, Some(value)))
            } else if matches!(option(arg), Ok(OptionArity::Operand)) {
                Some((arg.as_str(), values.next().map(String::as_str)))
            } else {
                Some((arg.as_str(), None))
            }
        })
    }

    pub fn arguments(&self) -> Vec<String> {
        self.jvm
            .iter()
            .cloned()
            .chain(self.entry.arguments())
            .chain(self.application.iter().cloned())
            .collect()
    }
}

/// Checks an isolated JVM region using exactly the same entry grammar as imports.
pub fn validate_jvm(arguments: &[String]) -> Result<()> {
    let mut probe = arguments.to_vec();
    probe.push("mcpm.validation.Main".into());
    let parsed = JavaCommand::parse(&probe)?;
    ensure!(
        parsed.jvm == arguments && parsed.application.is_empty(),
        "JVM arguments introduce another entry or consume the declared entry"
    );
    let mut files_enabled = true;
    for argument in arguments {
        ensure!(
            !files_enabled || !argument.starts_with('@'),
            "live Java @argument files are forbidden in configuration"
        );
        if argument == "--disable-@files" {
            files_enabled = false;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum OptionArity {
    Operand,
    Flag,
}

fn option(arg: &str) -> Result<OptionArity> {
    ensure!(
        arg != "--source" && !arg.starts_with("--source="),
        "Java source entries are unsupported"
    );
    let (name, inline) = arg
        .split_once('=')
        .map_or((arg, false), |(name, _)| (name, true));
    if matches!(
        name,
        "--describe-module"
            | "-d"
            | "-cp"
            | "-classpath"
            | "--class-path"
            | "-p"
            | "--module-path"
            | "--upgrade-module-path"
            | "--add-modules"
            | "--limit-modules"
            | "--add-reads"
            | "--add-exports"
            | "--add-opens"
            | "--patch-module"
            | "--enable-native-access"
            | "--illegal-native-access"
            | "--finalization"
    ) {
        return Ok(if inline {
            OptionArity::Flag
        } else {
            OptionArity::Operand
        });
    }
    if matches!(
        arg,
        "--dry-run"
            | "-version"
            | "--version"
            | "--list-modules"
            | "-help"
            | "--help"
            | "-h"
            | "-?"
            | "--help-extra"
            | "--validate-modules"
            | "-server"
            | "-client"
            | "-ea"
            | "-enableassertions"
            | "-da"
            | "-disableassertions"
            | "-esa"
            | "-enablesystemassertions"
            | "-dsa"
            | "-disablesystemassertions"
            | "-verbose"
            | "-showversion"
            | "--show-version"
            | "--enable-preview"
            | "--disable-@files"
            | "-noverify"
            | "-verify"
    ) || (inline && arg.starts_with("--"))
        || arg.starts_with("-D")
        || arg.starts_with("-X")
        || arg.starts_with("-javaagent:")
        || arg.starts_with("-agentlib:")
        || arg.starts_with("-agentpath:")
        || arg.starts_with("-verbose:")
        || arg.starts_with("-ea:")
        || arg.starts_with("-da:")
        || arg.starts_with("-enableassertions:")
        || arg.starts_with("-disableassertions:")
        || arg.starts_with("--illegal-access=")
    {
        return Ok(OptionArity::Flag);
    }
    bail!("unknown Java option {arg:?}; cannot determine its operand or entry boundary")
}

/// Expands Java argument files relative to the JVM working directory, without recursion.
pub fn expand(
    arguments: Vec<String>,
    directory: &Path,
    windows: bool,
) -> Result<(JavaCommand, Vec<PathBuf>)> {
    let mut flattened = Vec::new();
    let mut files = Vec::new();
    let mut disabled = false;
    let mut entry = false;
    let mut operand = false;
    let mut entry_operand = false;
    for arg in arguments {
        let values = if !disabled && !entry && arg.starts_with('@') {
            ensure!(
                !arg.starts_with("@@"),
                "escaped @ arguments before the entry are unsupported"
            );
            let value = if windows {
                arg[1..].replace('\\', "/")
            } else {
                arg[1..].to_owned()
            };
            let path = directory.join(value);
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read Java argument file {path:?}"))?;
            let values = argument_file(&text)
                .with_context(|| format!("failed to parse Java argument file {path:?}"))?;
            files.push(path);
            values
        } else {
            vec![arg]
        };
        for value in values {
            // Arguments read from an @file are never recursively expanded.
            if !entry {
                if entry_operand {
                    entry_operand = false;
                    entry = true;
                } else if operand {
                    operand = false;
                } else if value == "--disable-@files" {
                    disabled = true;
                } else if matches!(value.as_str(), "-jar" | "-m" | "--module") {
                    entry_operand = true;
                } else if !value.starts_with('-') || value.starts_with("--module=") {
                    entry = true;
                } else {
                    operand = matches!(option(&value)?, OptionArity::Operand);
                }
            }
            flattened.push(value);
        }
    }
    Ok((JavaCommand::parse(&flattened)?, files))
}

/// Java @file lexer: quotes, comments, quoted escapes and continued quoted lines.
pub fn argument_file(input: &str) -> Result<Vec<String>> {
    let mut chars = input.chars().peekable();
    let mut quote = None;
    let mut token = String::new();
    let mut active = false;
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        if quote.is_none() && ch == '#' {
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            if active {
                result.push(std::mem::take(&mut token));
                active = false;
            }
        } else if matches!(ch, '\r' | '\n') || (quote.is_none() && ch.is_ascii_whitespace()) {
            quote = None;
            if active {
                result.push(std::mem::take(&mut token));
                active = false;
            }
        } else if Some(ch) == quote {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            active = true;
        } else if quote.is_some() && ch == '\\' {
            let next = chars.next().context("unfinished Java argument escape")?;
            if matches!(next, '\r' | '\n') {
                if next == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                while chars.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
                    chars.next();
                }
            } else {
                token.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'f' => '\u{c}',
                    other => other,
                });
            }
            active = true;
        } else {
            token.push(ch);
            active = true;
        }
    }
    if active {
        result.push(token);
    }
    Ok(result)
}

/// Environment injection uses quote grouping, without @file comments or backslash escapes.
pub(crate) fn environment_options(input: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut active = false;
    for ch in input.chars() {
        ensure!(ch != '\0', "NUL in Java environment options");
        if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            active = true;
        } else if quote.is_none() && ch.is_ascii_whitespace() {
            if active {
                result.push(std::mem::take(&mut token));
                active = false;
            }
        } else {
            token.push(ch);
            active = true;
        }
    }
    ensure!(
        quote.is_none(),
        "unterminated quote in Java environment options"
    );
    if active {
        result.push(token);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn operand_paths_and_application_tokens_never_become_jvm_options() {
        for entry in [
            argv(&["example.Main"]),
            argv(&["-jar", "server.jar"]),
            argv(&["-m", "example/example.Main"]),
            argv(&["--module=example"]),
        ] {
            let mut values = argv(&[
                "-cp",
                "-Xmx2G",
                "-p",
                "modules",
                "--add-opens",
                "java.base/java.lang=ALL-UNNAMED",
                "-Xmx1G",
            ]);
            values.extend(entry.clone());
            values.extend(argv(&["-Xmx8G", "@literal", ""]));
            let parsed = JavaCommand::parse(&values).unwrap();
            assert_eq!(parsed.application, argv(&["-Xmx8G", "@literal", ""]));
            assert_eq!(parsed.jvm.len(), 7);
            assert_eq!(JavaCommand::parse(&parsed.arguments()).unwrap(), parsed);
        }
        for values in [
            argv(&["-Xlog:help", "example.Main"]),
            argv(&["--dry-run", "example.Main"]),
            argv(&["-XX:Flags=anything", "9-valid-at-JVM-layer"]),
            argv(&["--future-option=anything", "example.Main"]),
        ] {
            assert_eq!(JavaCommand::parse(&values).unwrap().arguments(), values);
        }
        for values in [
            argv(&["--source", "21", "Main.java"]),
            argv(&["Main.java"]),
            argv(&["--unknown", "value", "example.Main"]),
        ] {
            assert!(JavaCommand::parse(&values).is_err(), "{values:?}");
        }
    }

    #[test]
    fn argument_files_follow_entry_operands_and_disable_boundaries() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("entry"), "server.jar").unwrap();
        fs::write(root.path().join("jvm"), "-Xmx1G -cp 'folder with spaces'\n").unwrap();
        let (parsed, files) = expand(
            argv(&["@jvm", "-jar", "@entry", "@missing", "-Xmx8G"]),
            root.path(),
            false,
        )
        .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            parsed.entry,
            JavaEntry::Jar {
                path: "server.jar".into()
            }
        );
        assert_eq!(parsed.application, argv(&["@missing", "-Xmx8G"]));
        fs::write(root.path().join("entry"), "example/example.Main").unwrap();
        let (parsed, _) = expand(
            argv(&["--module", "@entry", "@literal"]),
            root.path(),
            false,
        )
        .unwrap();
        assert_eq!(
            parsed.entry,
            JavaEntry::Module {
                module: "example".into(),
                main_class: Some("example.Main".into())
            }
        );
        fs::write(root.path().join("jvm"), "-cp @nested example.Main").unwrap();
        let (parsed, _) = expand(argv(&["@jvm"]), root.path(), false).unwrap();
        assert!(validate_jvm(&parsed.jvm).is_err());
        let (parsed, _) = expand(
            argv(&["--disable-@files", "-cp", "@literal", "example.Main"]),
            root.path(),
            false,
        )
        .unwrap();
        assert_eq!(parsed.jvm, argv(&["--disable-@files", "-cp", "@literal"]));
        assert_eq!(
            argument_file("'a\\n' \"joined\\\n    line\" # comment\n\"new\nline\"").unwrap(),
            argv(&["a\n", "joinedline", "new", "line"])
        );
        assert_eq!(
            environment_options("-Dfoo='a\\nb' -Xmx1G").unwrap(),
            argv(&["-Dfoo=a\\nb", "-Xmx1G"])
        );
        assert!(validate_jvm(&environment_options("-Xmx1G # -Xmx2G").unwrap()).is_err());
    }
}
