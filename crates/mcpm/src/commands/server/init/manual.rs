//! Maps explicit init options to typed launch configuration and persistent conf overrides.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use clap::{ArgMatches, Args, Command, FromArgMatches};
use mcpm_lib::local::mc::server::config::{
    Arguments, ConfigurationDraft, ConfigurationOverrides, JavaEntry, LaunchConfig, Platform,
    Process,
};

#[derive(Debug, Args)]
struct RawManualArgs {
    /// Java entry: jar@PATH, class@MAIN_CLASS, or module@MODULE[:MAIN_CLASS].
    #[arg(long, value_parser = entry, requires_all = ["minecraft_version", "loader"], conflicts_with_all = ["remote", "start_script", "ignore_unknown_vars"], help_heading = "Manual launch configuration")]
    entry: Option<JavaEntry>,
    /// Source platform of the Java launch: unix or windows.
    #[arg(long, value_name = "PLATFORM", value_parser = launch_platform, requires = "entry", help_heading = "Manual launch configuration")]
    launch_platform: Option<Platform>,
    /// Java working directory relative to the server root.
    #[arg(
        long,
        value_name = "PATH",
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    launch_cwd: Option<String>,
    /// Set one Java process environment variable; repeat for multiple variables.
    #[arg(
        long,
        value_name = "NAME=VALUE",
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    launch_env: Vec<String>,
    /// Add JVM arguments split using shell quoting rules.
    #[arg(
        long,
        value_name = "ARGS",
        allow_hyphen_values = true,
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    java_args: Vec<String>,
    /// Add one JVM argument verbatim; repeat for multiple arguments.
    #[arg(
        long,
        value_name = "ARG",
        allow_hyphen_values = true,
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    java_arg: Vec<String>,
    /// Add application arguments split using shell quoting rules.
    #[arg(
        long,
        value_name = "ARGS",
        allow_hyphen_values = true,
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    app_args: Vec<String>,
    /// Add one application argument verbatim; repeat for multiple arguments.
    #[arg(
        long,
        value_name = "ARG",
        allow_hyphen_values = true,
        requires = "entry",
        help_heading = "Manual launch configuration"
    )]
    app_arg: Vec<String>,
    /// Set a conf.toml field by full path or shortcut name. Lists: = or += appends, := replaces.
    #[arg(long, value_name = "KEY=VALUE", requires_all = ["minecraft_version", "loader"], conflicts_with_all = ["remote", "start_script", "ignore_unknown_vars"], help_heading = "Manual settings")]
    conf: Vec<String>,
}

#[derive(Debug)]
pub(super) struct ManualArgs {
    raw: RawManualArgs,
    java: Vec<(usize, bool, String)>,
    app: Vec<(usize, bool, String)>,
}

impl ManualArgs {
    pub(super) fn build(self) -> Result<(Option<ConfigurationDraft>, ConfigurationOverrides)> {
        let mut overrides = ConfigurationOverrides::default();
        for setting in self.raw.conf {
            overrides.parse_with_aliases(&setting)?;
        }
        let Some(entry) = self.raw.entry else {
            return Ok((None, overrides));
        };
        let mut environment = BTreeMap::new();
        for assignment in self.raw.launch_env {
            let (name, value) = assignment
                .split_once('=')
                .context("--launch-env requires NAME=VALUE")?;
            ensure!(
                environment
                    .insert(name.to_owned(), value.to_owned())
                    .is_none(),
                "repeated --launch-env variable {name:?}"
            );
        }
        let launch = LaunchConfig {
            version: 1,
            platform: self.raw.launch_platform,
            process: Process {
                working_directory: self.raw.launch_cwd,
                environment,
            },
            jvm: Arguments {
                extra_arguments: argv(self.java, "--java-args")?,
            },
            entry,
            application: Arguments {
                extra_arguments: argv(self.app, "--app-args")?,
            },
        };
        Ok((
            Some(ConfigurationDraft {
                launch,
                imported: Vec::new(),
            }),
            overrides,
        ))
    }
}

impl FromArgMatches for ManualArgs {
    fn from_arg_matches(matches: &ArgMatches) -> clap::error::Result<Self> {
        let mut raw = RawManualArgs::from_arg_matches(matches)?;
        let java = ordered(
            matches,
            "java_args",
            std::mem::take(&mut raw.java_args),
            "java_arg",
            std::mem::take(&mut raw.java_arg),
        );
        let app = ordered(
            matches,
            "app_args",
            std::mem::take(&mut raw.app_args),
            "app_arg",
            std::mem::take(&mut raw.app_arg),
        );
        Ok(Self { raw, java, app })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> clap::error::Result<()> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for ManualArgs {
    fn augment_args(command: Command) -> Command {
        RawManualArgs::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        RawManualArgs::augment_args_for_update(command)
    }
}

fn ordered(
    matches: &ArgMatches,
    grouped_id: &str,
    grouped: Vec<String>,
    single_id: &str,
    single: Vec<String>,
) -> Vec<(usize, bool, String)> {
    let mut ordered = Vec::with_capacity(grouped.len() + single.len());
    for (id, split, values) in [(grouped_id, true, grouped), (single_id, false, single)] {
        if let Some(indices) = matches.indices_of(id) {
            ordered.extend(
                indices
                    .zip(values)
                    .map(|(index, value)| (index, split, value)),
            );
        }
    }
    ordered.sort_unstable_by_key(|(index, _, _)| *index);
    ordered
}

fn argv(input: Vec<(usize, bool, String)>, option: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for (_, split, value) in input {
        if split {
            result.extend(
                shlex::split(&value).with_context(|| format!("invalid quoting in {option}"))?,
            );
        } else {
            result.push(value);
        }
    }
    Ok(result)
}

fn entry(argument: &str) -> Result<JavaEntry> {
    let (kind, value) = argument
        .split_once('@')
        .context("entry must be jar@PATH, class@MAIN_CLASS, or module@MODULE[:MAIN_CLASS]")?;
    ensure!(!value.trim().is_empty(), "entry value must not be empty");
    Ok(match kind {
        "jar" => JavaEntry::Jar { path: value.into() },
        "class" => JavaEntry::Class {
            main_class: value.into(),
        },
        "module" => {
            let (module, main_class) = if let Some((module, class)) = value.split_once(':') {
                ensure!(
                    !module.trim().is_empty() && !class.trim().is_empty(),
                    "module and main class must not be empty"
                );
                (module, Some(class.into()))
            } else {
                (value, None)
            };
            JavaEntry::Module {
                module: module.into(),
                main_class,
            }
        }
        _ => bail!("unknown entry type {kind:?}; expected jar, class, or module"),
    })
}

fn launch_platform(value: &str) -> Result<Platform> {
    match value {
        "unix" => Ok(Platform::Unix),
        "windows" => Ok(Platform::Windows),
        _ => bail!("launch platform must be unix or windows"),
    }
}
