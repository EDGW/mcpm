//! Customization command dispatch; configuration operations belong to the theme module.

use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};

use super::output_style::{OutputStyle, text};
use crate::{
    terminal,
    theme::config::{self, Config},
};

mod preview;
mod settings;
mod tui;

#[derive(Args)]
#[command(
    after_help = "Modes:\n  TUI (default): select a Color Scheme, preview, create and manage schemes.\n  Console (--no-ui): scheme list/use/create/rename/delete, list, get, set.\n  Raw (--raw or redirected stdout): tab-separated names and values.\n\nExamples:\n  mcpm customize scheme create ocean --name Ocean\n  mcpm customize scheme use ocean\n  mcpm customize scheme rename ocean 'Ocean Blue'\n  mcpm customize scheme delete ocean\n  mcpm customize scheme use ocean\n  mcpm customize set 'TUI Dark Primary Color' Cyan\n\nConfiguration: ~/.mcpm; missing values use bundled defaults.\nNO_COLOR disables styling without preventing configuration changes."
)]
pub(crate) struct CustomizeArgs {
    /// Use command-based configuration instead of the interactive editor.
    #[arg(short = 'N', long)]
    no_ui: bool,
    #[command(subcommand)]
    command: Option<CustomizeCommand>,
    /// Color Theme to manage.
    #[arg(long, global = true, value_enum, default_value = "console-dark")]
    theme: Category,
}

#[derive(Subcommand)]
enum CustomizeCommand {
    /// List effective settings, including defaults (Console, Raw).
    List,
    /// List, select, create, rename or delete color schemes (Console, Raw).
    #[command(subcommand)]
    Scheme(SchemeCommand),
    /// Read one effective setting (Console, Raw).
    Get { key: String },
    /// Validate and persist one setting (Console, Raw).
    Set { key: String, value: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum Category {
    ConsoleDark,
    ConsoleLight,
    TuiDark,
    TuiLight,
}
impl Category {
    fn key(self) -> &'static str {
        match self {
            Self::TuiDark => "settings.tui_style_dark",
            Self::TuiLight => "settings.tui_style_light",
            Self::ConsoleDark => "settings.console_style_dark",
            Self::ConsoleLight => "settings.console_style_light",
        }
    }
}

#[derive(Subcommand)]
enum SchemeCommand {
    List,
    Use {
        id: String,
    },
    Create {
        /// Immutable identifier for the new theme.
        id: String,
        /// Display name; defaults to the ID.
        #[arg(long)]
        name: Option<String>,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
}

pub(crate) fn run(args: CustomizeArgs) -> Result<()> {
    let directory = config::directory()?;
    let mut config = crate::theme::get().config.clone();
    if args.command.is_none()
        && !args.no_ui
        && terminal::stdout_is_tty()
        && terminal::stderr_is_tty()
    {
        return tui::run(config, &directory);
    }
    let entries = match args.command.unwrap_or(CustomizeCommand::List) {
        CustomizeCommand::List => settings::fields(&config, true)
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect(),
        CustomizeCommand::Scheme(command) => {
            let mut changed = true;
            match command {
                SchemeCommand::List => changed = false,
                SchemeCommand::Use { id } => config.set(args.theme.key(), &id)?,
                SchemeCommand::Create { id, name } => {
                    config.create_palette(args.theme.key(), &id)?;
                    if let Some(name) = name {
                        config.rename_palette(&id, &name)?;
                    }
                }
                SchemeCommand::Rename { id, name } => config.rename_palette(&id, &name)?,
                SchemeCommand::Delete { id } => config.delete_palette(&id)?,
            }
            if changed {
                config.save_styles(&directory)?;
            }
            config
                .choices(args.theme.key())
                .into_iter()
                .map(|id| {
                    let name = config.palette_name(&id);
                    (id, name)
                })
                .collect()
        }
        CustomizeCommand::Get { key } => {
            let key = resolve(&config, &key);
            vec![
                config
                    .entries()
                    .into_iter()
                    .find(|(name, _)| name == &key)
                    .with_context(|| format!("unknown customization key {key:?}"))?,
            ]
        }
        CustomizeCommand::Set { key, value } => {
            let key = resolve(&config, &key);
            config.set(&key, &value)?;
            config.save_key(&directory, &key)?;
            vec![(key, value)]
        }
    };
    let labels = settings::fields(&config, true);
    let entries: Vec<_> = entries
        .into_iter()
        .map(|(key, value)| {
            labels
                .iter()
                .find(|field| field.key == key)
                .map(|field| (field.label.clone(), field.value.clone()))
                .unwrap_or_else(|| {
                    if let Some((id, field)) = key
                        .strip_prefix("styles.")
                        .and_then(|rest| rest.rsplit_once('.'))
                    {
                        (
                            format!(
                                "{} {}{}",
                                config.palette_name(id),
                                settings::title(field),
                                if field == "name" { "" } else { " Color" }
                            ),
                            crate::theme::Color::parse(&value)
                                .map(|color| color.tui_name().to_owned())
                                .unwrap_or(value),
                        )
                    } else {
                        (key, value)
                    }
                })
        })
        .collect();
    let style = OutputStyle::new();
    let mut output = io::stdout().lock();
    for (key, value) in entries {
        if terminal::stdout_is_tty() {
            writeln!(
                output,
                "{} = {}",
                style.muted(text(&key)),
                style.secondary(text(&value))
            )?;
        } else {
            writeln!(output, "{}\t{}", text(&key), text(&value))?;
        }
    }
    Ok(())
}

fn resolve(config: &Config, name: &str) -> String {
    settings::fields(config, true)
        .iter()
        .find(|field| field.label.eq_ignore_ascii_case(name))
        .map(|field| field.key.clone())
        .unwrap_or_else(|| name.to_owned())
}
