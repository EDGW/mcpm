//! Three-way Console decisions for metadata upgrades; redirected stdin is never consumed.

use std::io;

use anyhow::{Context, Result, ensure};
use console::{Key, Term};
use mcpm_backend::operations::server::upgrade::{UpgradeDecision, VanillaCandidate};

use mcpm_rich::rich;

use crate::{
    display::{
        rich::{Choice, line, prompt},
        style::{OutputStyle, text},
    },
    terminal,
};

pub(super) fn choose(candidate: &VanillaCandidate) -> Result<Option<UpgradeDecision>> {
    ensure!(
        terminal::stdout_is_tty() && terminal::stderr_is_tty(),
        "Vanilla evidence requires Console confirmation; run info --upgrade in a terminal without --raw; registration was not changed"
    );
    let term = Term::stderr();
    let style = OutputStyle::stderr();
    term.write_line(&format!(
        "{} {}",
        style.muted("Detected Vanilla entry:"),
        style.secondary(text(&candidate.main_class))
    ))?;
    if let Some(jar) = &candidate.jar {
        term.write_line(&format!(
            "{} {}",
            style.muted("Launch JAR:"),
            style.muted(text(&jar.display().to_string()))
        ))?;
    }
    term.write_line("")?;
    let vanilla = style.loader("vanilla", "Vanilla");
    term.write_line(&format!(
        // Loader colors are per-loader, not a role, so the name is spliced between rich lines.
        "{} {vanilla}{}",
        line(&rich!("`Yes` changes the loader to"), &style),
        line(
            &rich!(
                "; `no` keeps [Unrecognized](warning); [Cancel](primary) leaves the registration unchanged."
            ),
            &style
        ),
    ))?;
    term.write_line("")?;
    loop {
        let choices = [
            Choice {
                label: "y(es)",
                default: false,
            },
            Choice {
                label: "n(o)",
                default: false,
            },
            Choice {
                label: "C(ancel)",
                default: true,
            },
        ];
        term.write_str(&format!(
            "{} {vanilla}{}",
            line(&rich!("Set loader to"), &style),
            prompt(&rich!("?"), &choices, &style),
        ))
        .context("failed to write upgrade confirmation")?;
        let answer = read_answer(&term).context("failed to read upgrade confirmation")?;
        term.write_line("")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(Some(UpgradeDecision::UseVanilla)),
            "n" | "no" => return Ok(Some(UpgradeDecision::Preserve)),
            "" | "c" | "cancel" => return Ok(None),
            _ => {
                term.write_line(&line(
                    &rich!("[Please enter y(es), n(o), or C(ancel).](warning)"),
                    &style,
                ))?;
                term.write_line("")?;
            }
        }
    }
}

pub(super) fn cancelled() -> Result<()> {
    let term = Term::stderr();
    term.write_line(&line(
        &rich!("[Upgrade cancelled; registration was not changed.](warning)"),
        &OutputStyle::stderr(),
    ))
    .context("failed to write upgrade cancellation")?;
    term.write_line("")
        .context("failed to separate upgrade cancellation")
}

pub(super) fn upgraded(from: u32, to: u32) -> Result<()> {
    let term = Term::stderr();
    term.write_line(&line(
        &rich!("[Upgraded server registration from version {from} to {to}.](success)"),
        &OutputStyle::stderr(),
    ))
    .context("failed to write upgrade result")?;
    term.write_line("")
        .context("failed to separate upgrade result")
}

fn read_answer(term: &Term) -> io::Result<String> {
    let mut answer = String::new();
    loop {
        let key = match term.read_key() {
            Ok(key) => key,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::Interrupted
                ) =>
            {
                term.write_line("")?;
                return Ok(String::new());
            }
            Err(error) => return Err(error),
        };
        match key {
            Key::Enter => {
                term.write_line("")?;
                return Ok(answer);
            }
            Key::Escape | Key::Char('\u{3}' | '\u{4}' | '\u{1a}') => {
                term.write_line("")?;
                return Ok(String::new());
            }
            Key::Char(ch) if ch.is_ascii_alphabetic() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            Key::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}
