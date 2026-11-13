//! Terminal-only consent after fetching the current official EULA with the selected sources.

use anyhow::{Context, Result, ensure};
use mcpm_lib::download::{DownloadConfig, eula, sources};
use mcpm_rich::rich;

use crate::{
    commands::confirm,
    display::{
        rich::{Choice, line, panel, prompt},
        style::OutputStyle,
    },
    progress::Progress,
    terminal,
};

pub(super) async fn accept(config: &DownloadConfig) -> Result<bool> {
    ensure!(
        terminal::stdout_is_tty() && terminal::stderr_is_tty(),
        "--eula requires an interactive terminal; omit --eula to install without accepting the EULA"
    );
    let progress = Progress::new("Fetching Minecraft EULA...", "Fetching EULA");
    let preview = eula::fetch(&sources::downloader(config)?).await?;
    drop(progress);
    let term = console::Term::stderr();
    let style = OutputStyle::stderr();
    let url = preview.url;
    let mut content = rich!("`{url}`");
    for value in &preview.lines {
        let body = &value.text;
        content.extend(if value.heading {
            rich!("[{body}](primary)")
        } else {
            rich!("{body}")
        });
    }
    let remaining = preview.remaining_lines;
    content.extend(rich!("[(+ {remaining} lines)](muted)"));
    panel(
        &mut std::io::stderr().lock(),
        " Minecraft EULA ",
        &content,
        term.size().1,
        &style,
    )?;
    term.write_line("")?;
    let choices = [
        Choice {
            label: "y",
            default: false,
        },
        Choice {
            label: "N",
            default: true,
        },
    ];
    term.write_str(&prompt(
        &rich!("[Do you accept the Minecraft EULA?](important)"),
        &choices,
        &style,
    ))?;
    let answer = confirm::read_answer(&term, true).context("failed to read EULA confirmation")?;
    let accepted = matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if !accepted {
        term.write_line(&line(
            &rich!("EULA not accepted; server creation cancelled."),
            &style,
        ))?;
    }
    Ok(accepted)
}
