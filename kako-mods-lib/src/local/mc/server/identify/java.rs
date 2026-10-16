use super::{
    archive, script,
    tokens::{self, Syntax},
};
use anyhow::{Context, Result, ensure};
use std::{collections::HashMap, fs, path::PathBuf};

/// A Java launch parsed without executing the script. Paths are absolute and resolved
/// against the statically known JVM working directory. Application and JVM arguments
/// are separated so application text can never masquerade as JVM options.
#[derive(Debug, Clone)]
pub struct ServerLaunch {
    pub executable: String,
    pub script: PathBuf,
    pub line: usize,
    pub directory: PathBuf,
    pub arguments: Vec<String>,
    pub argument_files: Vec<PathBuf>,
    pub main_class: String,
    pub jar: Option<PathBuf>,
    pub classpath: Vec<PathBuf>,
    pub properties: HashMap<String, String>,
    pub game_arguments: Vec<String>,
    /// Separator used by the script's target Java platform, including offline inspection.
    pub path_separator: char,
}

impl ServerLaunch {
    /// One unambiguous game option, accepting both `--name value` and `--name=value`.
    pub fn option(&self, name: &str) -> Result<Option<&str>> {
        let mut found = None;
        for (index, arg) in self.game_arguments.iter().enumerate() {
            let value = if arg == name {
                Some(
                    self.game_arguments
                        .get(index + 1)
                        .with_context(|| format!("missing game option value for {name}"))?
                        .as_str(),
                )
            } else {
                arg.strip_prefix(&format!("{name}="))
            };
            if let Some(value) = value {
                ensure!(
                    !value.trim().is_empty() && !value.starts_with('-'),
                    "invalid game option value for {name}"
                );
                ensure!(found.is_none(), "duplicate game option {name}");
                found = Some(value);
            }
        }
        Ok(found)
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        self.directory.join(if self.path_separator == ';' {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        })
    }
}

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
    let mut classpath_seen = false;
    let mut modulepath_seen = false;
    while i < arguments.len() {
        let arg = &arguments[i];
        if arg == "-jar" {
            i += 1;
            let path = launch.resolve(arguments.get(i).context("-jar has no path")?);
            tracing::debug!(?path, "reading launch JAR manifest to determine Java entry");
            let manifest = archive::manifest(&path)?;
            launch.main_class = manifest
                .get("main-class")
                .with_context(|| format!("launch JAR {path:?} has no Main-Class"))?
                .clone();
            // -jar ignores explicit JVM classpath; only the manifest applies.
            launch.classpath = archive::manifest_paths(&path, &manifest)?;
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
            ensure!(!key.is_empty(), "empty JVM property name");
            launch.properties.insert(key.into(), value.into());
        } else if arg.starts_with("-X") {
            // JVM tuning flags may contain '='; they do not consume another argv item.
        } else if let Some((name, value)) = arg.split_once('=') {
            if is_classpath(name) {
                mark_classpath(name, &mut classpath_seen, &mut modulepath_seen)?;
                add_paths(&mut launch, value);
            }
        } else if is_operand(arg) {
            i += 1;
            let value = arguments
                .get(i)
                .with_context(|| format!("missing JVM option operand for {arg}"))?;
            if is_classpath(arg) {
                mark_classpath(arg, &mut classpath_seen, &mut modulepath_seen)?;
                add_paths(&mut launch, value);
            }
        }
        // Unknown JVM options are retained verbatim in the flattened command.
        // Recognition concerns loader/game evidence, not JVM option validity.
        i += 1;
    }
    let end = entry.context("Java command has no server entry point")?;
    launch.game_arguments = arguments[end..].to_vec();
    Ok(launch)
}

fn mark_classpath(option: &str, classpath: &mut bool, modulepath: &mut bool) -> Result<()> {
    let seen = if matches!(option, "-p" | "--module-path") {
        modulepath
    } else {
        classpath
    };
    ensure!(
        !*seen,
        "repeated Java classpath/module-path options are unsupported"
    );
    *seen = true;
    Ok(())
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
fn add_paths(launch: &mut ServerLaunch, value: &str) {
    for path in value.split(launch.path_separator) {
        let path = launch.resolve(path);
        if !launch.classpath.contains(&path) {
            launch.classpath.push(path);
        }
    }
}

pub(super) fn vanilla(launch: &ServerLaunch) -> Result<Option<String>> {
    let Some(jar) = &launch.jar else {
        return Ok(None);
    };
    if !matches!(
        launch.main_class.as_str(),
        "net.minecraft.bundler.Main"
            | "net.minecraft.server.Main"
            | "net.minecraft.server.MinecraftServer"
    ) {
        return Ok(None);
    }
    ensure!(
        launch.classpath.is_empty(),
        "vanilla launch with extra manifest classpath is unverified"
    );
    let class = format!("{}.class", launch.main_class.replace('.', "/"));
    ensure!(archive::has(jar, &class)?, "vanilla entry class is missing");
    let version = archive::minecraft_version(jar)?
        .context("vanilla JAR lacks verifiable Minecraft version metadata")?;
    Ok(Some(version))
}
