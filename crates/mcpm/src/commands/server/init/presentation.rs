//! Shared candidate summaries for interactive selection and noninteractive diagnostics.

use mcpm_backend::operations::server::init::Candidate;
use mcpm_exec::error::TippedError;
use mcpm_lib::local::mc::server::config::{ImportedValue, JavaEntry, LaunchConfig, Platform};
use mcpm_rich::{Role, rich};

use crate::display::style::text;

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

/// Console candidate details with semantic styles for each field.
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

/// Replays the selected identity, launch structure, and settings at the original CLI target.
pub(super) fn manual_command(
    prefix: &str,
    path: &str,
    candidate: &Candidate,
    replace_conf: bool,
) -> Option<String> {
    let mut command = prefix.to_owned();
    let metadata = &candidate.metadata;
    append_option(
        &mut command,
        "--minecraft-version",
        &metadata.minecraft_version,
    )?;
    if let Some(loader) = metadata.loader.declaration() {
        append_option(&mut command, "--loader", &loader.loader_type)?;
        append_option(&mut command, "--loader-version", &loader.version)?;
    } else {
        command.push_str(" --loader vanilla");
    }
    if let Some(draft) = &candidate.configuration {
        append_launch_options(&mut command, &draft.launch)?;
        if !replace_conf && !draft.imported.is_empty() {
            command.push_str(" --preserve-conf");
        }
        for setting in &draft.imported {
            append_option(&mut command, "--conf", &setting.override_argument().ok()?)?;
        }
    }
    if path.starts_with('-') {
        command.push_str(" --");
    }
    command.push(' ');
    append_shell_arg(&mut command, path)?;
    Some(command)
}

fn append_launch_options(command: &mut String, launch: &LaunchConfig) -> Option<()> {
    let entry = match &launch.entry {
        JavaEntry::Jar { path } if !path.trim().is_empty() => format!("jar@{path}"),
        JavaEntry::Class { main_class } if !main_class.trim().is_empty() => {
            format!("class@{main_class}")
        }
        JavaEntry::Module { module, main_class }
            if !module.trim().is_empty() && !module.contains(':') =>
        {
            match main_class {
                Some(class) if !class.trim().is_empty() => format!("module@{module}:{class}"),
                Some(_) => return None,
                None => format!("module@{module}"),
            }
        }
        _ => return None,
    };
    append_option(command, "--entry", &entry)?;
    if let Some(platform) = launch.platform {
        let platform = match platform {
            Platform::Unix => "unix",
            Platform::Windows => "windows",
        };
        append_option(command, "--launch-platform", platform)?;
    }
    if let Some(directory) = &launch.process.working_directory {
        append_option(command, "--launch-cwd", directory)?;
    }
    for (key, value) in &launch.process.environment {
        append_option(command, "--launch-env", &format!("{key}={value}"))?;
    }
    for argument in &launch.jvm.extra_arguments {
        append_option(command, "--java-arg", argument)?;
    }
    for argument in &launch.application.extra_arguments {
        append_option(command, "--app-arg", argument)?;
    }
    Some(())
}

pub(super) fn append_option(command: &mut String, name: &str, value: &str) -> Option<()> {
    command.push(' ');
    if value.is_empty() || value.starts_with('-') {
        append_shell_arg(command, &format!("{name}={value}"))
    } else {
        command.push_str(name);
        command.push(' ');
        append_shell_arg(command, value)
    }
}

fn append_shell_arg(command: &mut String, value: &str) -> Option<()> {
    // Keep each command on one printable terminal line without changing its arguments.
    if value.chars().any(char::is_control) {
        return None;
    }
    #[cfg(not(windows))]
    command.push_str(&shlex::try_quote(value).ok()?);
    #[cfg(windows)]
    append_powershell_arg(command, value);
    Some(())
}

#[cfg(windows)]
fn append_powershell_arg(command: &mut String, value: &str) {
    command.push('\'');
    for ch in value.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            command.push(ch);
        }
        command.push(ch);
    }
    command.push('\'');
}

pub(super) fn ambiguous(candidates: &[Candidate]) -> anyhow::Error {
    let mut tip = rich!(
        "Run init in an interactive terminal without --raw to choose a launch, or supply complete version and loader options to initialize manually without importing a startup script."
    );
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
        tip.extend(rich!(
            "{identity}: `{location}`\n\nMain: {main}\n\nEntry: {entry}\n\nArguments: {application}"
        ));
    }
    TippedError::new(
        anyhow::anyhow!(
            "multiple identified server launches found; selection requires an interactive terminal"
        ),
        Some(tip),
        None,
    )
    .into()
}
