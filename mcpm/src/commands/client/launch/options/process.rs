use std::ffi::OsString;

use clap::{Args, ValueEnum};
use kako_mods_lib::local::mc::client::{
    CheckOptionValue, CheckOptions,
    launch::{LaunchOptions, ProcessStream},
};

#[derive(Clone, Copy, ValueEnum)]
enum GameOutput {
    Inherit,
    Null,
}
#[derive(Clone, Copy, ValueEnum)]
enum Verify {
    None,
    Default,
    Strict,
    Shallow,
}

#[derive(Args)]
#[command(next_help_heading = "Process")]
pub(super) struct ProcessArgs {
    /// Child stdout/stderr handling; null suppresses game logs.
    #[arg(long, value_enum, default_value = "inherit")]
    game_output: GameOutput,
    /// Clear inherited child environment before applying --env entries.
    #[arg(long)]
    clear_env: bool,
    /// Set a child environment variable; repeatable.
    #[arg(long, value_name = "KEY=VALUE", value_parser = assignment)]
    env: Vec<(OsString, OsString)>,
    /// Remove a child environment variable; removals take precedence over --env.
    #[arg(long, value_name = "KEY")]
    unset_env: Vec<OsString>,
    /// Optional resource checks before launch; strict hashes all, shallow checks sizes.
    #[arg(long, value_enum, default_value = "none")]
    verify: Verify,
}

fn assignment(value: &str) -> Result<(OsString, OsString), String> {
    let (key, value) = value.split_once('=').ok_or("expected KEY=VALUE")?;
    if key.is_empty() || key.contains('\0') {
        return Err("expected a nonempty environment key without NUL".into());
    }
    Ok((key.into(), value.into()))
}

impl ProcessArgs {
    pub(super) fn apply(&self, options: &mut LaunchOptions) {
        options.process.clear_environment = self.clear_env;
        options.process.stdin = ProcessStream::Null;
        let stream = match self.game_output {
            GameOutput::Inherit => ProcessStream::Inherit,
            GameOutput::Null => ProcessStream::Null,
        };
        options.process.stdout = stream;
        options.process.stderr = stream;
        for (key, value) in &self.env {
            options
                .process
                .environment
                .insert(key.clone(), Some(value.clone()));
        }
        for key in &self.unset_env {
            options.process.environment.insert(key.clone(), None);
        }
        options.verification = match self.verify {
            Verify::None => None,
            Verify::Default => Some(CheckOptions::default()),
            Verify::Strict | Verify::Shallow => {
                let mode = if matches!(self.verify, Verify::Strict) {
                    CheckOptionValue::Hash
                } else {
                    CheckOptionValue::Size
                };
                Some(CheckOptions {
                    libraries: mode,
                    assets: mode,
                    asset_index: mode,
                    version_jar: mode,
                    logging: mode,
                })
            }
        };
    }
}
