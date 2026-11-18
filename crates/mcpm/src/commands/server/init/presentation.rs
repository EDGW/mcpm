//! Shared candidate summaries for interactive selection and noninteractive diagnostics.

use mcpm_backend::operations::server::init::Candidate;
use mcpm_exec::error::TippedError;
use mcpm_lib::local::mc::server::config::ImportedValue;
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
