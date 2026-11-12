//! Candidate summaries and shell-safe retry commands; no launch parsing or host path resolution.

use std::path::Path;

use anyhow::{Context, Result};
use mcpm_backend::operations::server::init::Candidate;
use mcpm_exec::error::TippedError;
use mcpm_lib::local::mc::server::config::ImportedValue;
use mcpm_rich::{Role, rich};

use super::InitArgs;
use crate::{display::style::text, program, ssh::SshArgs, terminal};

/// Identity, source, main class, entry, and application summary from the existing draft.
pub(super) fn summary(candidate: &Candidate) -> Summary {
    let loader = &candidate.metadata.loader;
    let identity = match loader.declaration() {
        Some(loader) => format!(
            "{} {} / Minecraft {}",
            loader.loader_type, loader.version, candidate.metadata.minecraft_version
        ),
        None => format!(
            "Vanilla / Minecraft {}",
            candidate.metadata.minecraft_version
        ),
    };
    let (entry, application) = match &candidate.configuration {
        Some(draft) => {
            let entry = draft
                .launch
                .entry
                .arguments()
                .iter()
                .map(|arg| format!("{arg:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            let mut arguments = draft
                .launch
                .application
                .extra_arguments
                .iter()
                .take(6)
                .map(|arg| format!("{arg:?}"))
                .collect::<Vec<_>>();
            if draft.launch.application.extra_arguments.len() > 6 {
                arguments.push(format!(
                    "… (+{} arguments)",
                    draft.launch.application.extra_arguments.len() - 6
                ));
            }
            for setting in &draft.imported {
                if setting.field.starts_with("server.") {
                    let value = match &setting.value {
                        ImportedValue::String(value) => format!("{value:?}"),
                        ImportedValue::Boolean(value) => value.to_string(),
                        ImportedValue::Integer(value) => value.to_string(),
                        ImportedValue::Arguments(value) => format!("{value:?}"),
                    };
                    arguments.push(format!("{}={value}", setting.field));
                }
            }
            (
                entry,
                if arguments.is_empty() {
                    "(none)".into()
                } else {
                    arguments.join(" ")
                },
            )
        }
        None => ("unavailable".into(), "unavailable".into()),
    };
    Summary {
        identity: text(&identity),
        script: format!("{}:", text(&candidate.script.display().to_string())),
        line: candidate.line.to_string(),
        main: text(&candidate.main_class),
        entry: text(&entry),
        application: text(&application),
    }
}

/// Display values stay separate so Console and TUI share the same semantic roles.
pub(super) struct Summary {
    pub identity: String,
    script: String,
    line: String,
    main: String,
    entry: String,
    application: String,
}
impl Summary {
    pub(super) fn details(&self) -> [[(Role, &str); 2]; 4] {
        [
            [(Role::Normal, &self.script), (Role::Primary, &self.line)],
            [(Role::Normal, "Main: "), (Role::Code, &self.main)],
            [(Role::Normal, "Entry: "), (Role::Code, &self.entry)],
            [
                (Role::Normal, "Arguments: "),
                (Role::Code, &self.application),
            ],
        ]
    }
}

/// Keep the invocation's target and write intent, before workspace routing consumes PATH.
pub(super) fn retry_base(args: &InitArgs, ssh: &SshArgs) -> Result<String> {
    let mut command = program::roots().server.command("init");
    let mut append = |option: &str, value: Option<&str>| -> Result<()> {
        command.push(' ');
        command.push_str(option);
        if let Some(value) = value {
            command.push('=');
            command.push_str(&quote(value)?);
        }
        Ok(())
    };
    if let Some(connection) = ssh.explicit_connection()? {
        append("--ssh", Some(&connection.ssh))?;
        if let Some(path) = connection.mcpmd {
            append("--ssh-mcpmd", Some(&path))?;
        }
    }
    for (enabled, option) in [
        (args.force, "--force"),
        (args.foverride_conf, "--foverride-conf"),
        (args.no_git, "--no-git"),
        (args.git, "--git"),
        (args.no_ui, "--no-ui"),
        (args.full_command, "--full-command"),
        (terminal::is_raw(), "--raw"),
    ] {
        if enabled {
            append(option, None)?;
        }
    }
    // Explicit launch selection replaces partial identity filters, which conflict with --start-script.
    Ok(command)
}

pub(super) fn ambiguous(
    candidates: &[Candidate],
    base: &str,
    path: &Path,
) -> Result<anyhow::Error> {
    let mut tip =
        rich!("Select one launch using its exact source line. Copy one of these commands:");
    for candidate in candidates {
        let Summary {
            identity,
            script,
            line,
            main,
            entry,
            application,
        } = summary(candidate);
        let location = format!("{script}{line}");
        let selected = if candidate.script == candidate.start_script {
            candidate.line.to_string()
        } else {
            format!("{}:{}", candidate.script.display(), candidate.line)
        };
        let command = format!(
            "{base} --start-script={} --start-line={} -- {}",
            quote_path(&candidate.start_script)?,
            quote(&selected)?,
            quote_path(path)?
        );
        tip.extend(rich!(
            "{identity}: `{location}`\n\nMain: {main}\n\nEntry: {entry}\n\nArguments: {application}\n\n`{command}`"
        ));
    }
    Ok(TippedError::new(anyhow::anyhow!("multiple identified server launches found; select a command with --start-script and --start-line"), Some(tip), None).into())
}

fn quote_path(path: &Path) -> Result<String> {
    quote(
        path.to_str()
            .context("candidate script path is not UTF-8")?,
    )
}

/// POSIX shell on Unix; PowerShell on Windows (including embedded single quotes).
fn quote(value: &str) -> Result<String> {
    #[cfg(windows)]
    {
        anyhow::ensure!(!value.contains('\0'), "NUL in retry command argument");
        Ok(format!("'{}'", value.replace('\'', "''")))
    }
    #[cfg(not(windows))]
    {
        shlex::try_quote(value)
            .map(|value| value.into_owned())
            .context("cannot quote retry command argument")
    }
}
