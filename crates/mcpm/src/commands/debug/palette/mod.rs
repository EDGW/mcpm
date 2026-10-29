//! Palette mode selection; raw output intentionally has no content.

use std::io;

use anyhow::{Context, Result};
use clap::Args;

use crate::terminal;

mod output;
mod tui;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes:\n  TUI (default): crossterm color names with Normal / Bold / Dim samples.\n  Console (--no-ui): owo-colors expressions with the same samples.\n  Raw (--raw or redirected stdout): no output.\n\nThe palette contains 8 ordinary colors, 8 bright colors, and the default\nforeground. Colors follow the terminal theme; NO_COLOR disables colors."
)]
pub(super) struct PaletteArgs {
    /// Print the palette to the console instead of opening the interactive TUI.
    #[arg(short = 'N', long)]
    no_ui: bool,
}

pub(super) fn run(args: PaletteArgs) -> Result<()> {
    if terminal::is_raw() || !terminal::stdout_is_tty() {
        return Ok(());
    }
    if !args.no_ui && terminal::stderr_is_tty() {
        tui::run().context("failed to display the palette TUI")
    } else {
        output::write(&mut io::stdout().lock()).context("failed to write the console palette")
    }
}
