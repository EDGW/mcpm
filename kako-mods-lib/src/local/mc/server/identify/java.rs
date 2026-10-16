//! Static parsing of Java commands and argument files into server launch evidence.

use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::{Context, Result, ensure};

use crate::local::{java::jar, mc::server::ServerLaunch};

use super::{
    script,
    tokens::{self, Syntax},
};

pub(super) fn parse(
    directory: PathBuf,
    script: PathBuf,
    line: usize,
    syntax: Syntax,
    executable: String,
    raw: Vec<String>,
) -> Result<ServerLaunch> {
    let mut arguments = Vec::new();
    let mut argument_files = Vec::new();
    let mut options = true;
    let mut expect_operand = false;
    let mut expand_files = true;
    for arg in raw {
        // The Java launcher expands @files only before its main entry point.
        if options && expand_files && arg.starts_with('@') {
            ensure!(
                !arg.starts_with("@@"),
                "escaped @ arguments are unsupported"
            );
            let path = script::resolve(&directory, &arg[1..], syntax);
            tracing::debug!(?path, "expanding Java argument file");
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read Java argument file {path:?}"))?;
            let values = tokens::split(&text, Syntax::Java, &HashMap::new(), &script)
                .with_context(|| format!("failed to parse Java argument file {path:?}"))?;
            ensure!(
                !values.iter().any(|v| v.starts_with('@')),
                "nested Java @argument files are unsupported in {path:?}"
            );
            argument_files.push(path);
            for value in values {
                if value == "--disable-@files" {
                    expand_files = false;
                }
                track_entry(&value, &mut options, &mut expect_operand);
                arguments.push(value);
            }
        } else {
            if arg == "--disable-@files" {
                expand_files = false;
            }
            track_entry(&arg, &mut options, &mut expect_operand);
            arguments.push(arg);
        }
    }
    let mut launch = ServerLaunch {
        executable,
        script,
        line,
        directory,
        arguments: arguments.clone(),
        argument_files,
        main_class: String::new(),
        jar: None,
        classpath: Vec::new(),
        properties: HashMap::new(),
        game_arguments: vec![],
        path_separator: if syntax == Syntax::Batch { ';' } else { ':' },
    };
    let mut i = 0;
    let mut entry = None;
    let mut classpath = Vec::new();
    let mut modulepath = Vec::new();
    while i < arguments.len() {
        let arg = &arguments[i];
        if arg == "-jar" {
            i += 1;
            let path = launch.resolve(arguments.get(i).context("-jar has no path")?);
            tracing::debug!(?path, "reading launch JAR manifest to determine Java entry");
            let manifest = jar::manifest(&path)?;
            launch.main_class = manifest
                .get("main-class")
                .with_context(|| format!("launch JAR {path:?} has no Main-Class"))?
                .clone();
            // -jar ignores explicit JVM classpath; only the manifest applies.
            launch.classpath = jar::manifest_paths(&path, &manifest);
            launch.jar = Some(path);
            entry = Some(i + 1);
            break;
        }
        if !arg.starts_with('-') {
            ensure!(!arg.is_empty(), "empty Java main class");
            launch.main_class = arg.clone();
            entry = Some(i + 1);
            break;
        }
        if let Some(property) = arg.strip_prefix("-D") {
            let (key, value) = property.split_once('=').unwrap_or((property, ""));
            if !key.is_empty() {
                launch.properties.insert(key.into(), value.into());
            }
        } else if arg.starts_with("-X") {
            // JVM tuning flags may contain '='; they do not consume another argv item.
        } else if let Some((name, value)) = arg.split_once('=') {
            if is_classpath(name) {
                let paths = if matches!(name, "-p" | "--module-path") {
                    &mut modulepath
                } else {
                    &mut classpath
                };
                *paths = paths_from(&launch, value);
            }
        } else if is_operand(arg) {
            i += 1;
            let value = arguments
                .get(i)
                .with_context(|| format!("missing JVM option operand for {arg}"))?;
            if is_classpath(arg) {
                let paths = if matches!(arg.as_str(), "-p" | "--module-path") {
                    &mut modulepath
                } else {
                    &mut classpath
                };
                *paths = paths_from(&launch, value);
            }
        }
        // Unknown JVM options are retained verbatim in the flattened command.
        // Recognition concerns loader/game evidence, not JVM option validity.
        i += 1;
    }
    let end = entry.context("Java command has no server entry point")?;
    launch.game_arguments = arguments[end..].to_vec();
    if launch.jar.is_none() {
        launch.classpath = classpath;
        for path in modulepath {
            if !launch.classpath.contains(&path) {
                launch.classpath.push(path);
            }
        }
    }
    Ok(launch)
}

fn is_operand(option: &str) -> bool {
    matches!(
        option,
        "-cp"
            | "-classpath"
            | "--class-path"
            | "-p"
            | "--module-path"
            | "--add-modules"
            | "--add-opens"
            | "--add-exports"
            | "--enable-native-access"
            | "--limit-modules"
            | "--upgrade-module-path"
    )
}
fn is_classpath(option: &str) -> bool {
    matches!(
        option,
        "-cp" | "-classpath" | "--class-path" | "-p" | "--module-path"
    )
}
fn track_entry(arg: &str, options: &mut bool, operand: &mut bool) {
    if !*options {
        return;
    }
    if *operand {
        *operand = false;
        return;
    }
    if arg == "-jar" || !arg.starts_with('-') {
        *options = false;
    } else if is_operand(arg) {
        *operand = true;
    }
}
/// Repeated options replace their own path category without mixing overridden evidence.
fn paths_from(launch: &ServerLaunch, value: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in value.split(launch.path_separator) {
        let path = launch.resolve(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}
