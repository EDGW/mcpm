//! Terminal file release summaries and escaped TSV records for remote projects.

use std::io::{self, Write};

use mcpm_backend::source::{FilePage, ProjectFile, ProjectRef, ReleaseType};

use crate::{
    display::style::{OutputStyle, text},
    terminal,
};

/// TSV columns: source, project ID, file ID, name, version, release type,
/// Minecraft versions, loaders, filename, bytes, download URL.
pub(super) fn write_files(
    output: &mut impl Write,
    project: &ProjectRef,
    page: &FilePage,
    selected_file: Option<&ProjectFile>,
    columns: &super::columns::ColumnArgs,
) -> io::Result<()> {
    let formatted = terminal::stdout_is_tty();
    let style = OutputStyle::new();
    let width = textwrap::termwidth().saturating_sub(6).max(1);
    if formatted && !columns.is_explicit() {
        writeln!(
            output,
            "{}",
            style.primary(format!(
                "{} / {}",
                text(&project.source.0),
                text(&project.id.0)
            ))
        )?;
        let total = page
            .total
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into());
        writeln!(
            output,
            "{}",
            style.muted(format!(
                "{} files shown · offset {} · {total} matching files",
                page.files.len(),
                page.offset
            ))
        )?;
        if page.files.is_empty() && selected_file.is_none() {
            writeln!(output, "No matching files.")?;
        }
    }
    let files = selected_file
        .into_iter()
        .map(|file| (None, file))
        .chain(super::other_files(page, selected_file).map(|(index, file)| (Some(index), file)));
    for (index, file) in files {
        let is_selected = index.is_none();
        let number = index.map(|index| page.offset + index as u64 + 1);
        let channel = match file.release_type {
            ReleaseType::Release => "release",
            ReleaseType::Beta => "beta",
            ReleaseType::Alpha => "alpha",
        };
        if columns.is_explicit() {
            use super::columns::Column;
            let selected = columns.selected();
            let values: Vec<_> = selected
                .iter()
                .map(|column| {
                    text(&match column {
                        Column::Index => {
                            number.map(|number| number.to_string()).unwrap_or_default()
                        }
                        Column::Selected => is_selected.to_string(),
                        Column::Source => project.source.0.clone(),
                        Column::Project => project.id.0.clone(),
                        Column::Id => file.reference.id.0.clone(),
                        Column::Name => file.name.clone(),
                        Column::Version => file.version.clone().unwrap_or_default(),
                        Column::Release => channel.into(),
                        Column::GameVersions => file.game_versions.join(","),
                        Column::Loaders => file.loaders.join(","),
                        Column::Filename => file.filename.clone(),
                        Column::Size => file.size.to_string(),
                        Column::Url => file
                            .download_url
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    })
                })
                .collect();
            if formatted {
                let values: Vec<_> = selected
                    .iter()
                    .zip(values)
                    .map(|(column, value)| match column {
                        Column::Name => style.primary(value),
                        Column::Version | Column::GameVersions => style.secondary(value),
                        Column::Loaders => file
                            .loaders
                            .iter()
                            .map(|loader| style.loader(loader, &text(loader)))
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => style.muted(value),
                    })
                    .collect();
                for line in textwrap::wrap(&values.join("  "), width) {
                    writeln!(output, "  {line}")?;
                }
            } else {
                writeln!(output, "{}", values.join("\t"))?;
            }
            continue;
        }
        if !formatted {
            let values = [
                project.source.0.clone(),
                project.id.0.clone(),
                file.reference.id.0.clone(),
                file.name.clone(),
                file.version.clone().unwrap_or_default(),
                channel.into(),
                file.game_versions.join(","),
                file.loaders.join(","),
                file.filename.clone(),
                file.size.to_string(),
                file.download_url
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            ];
            writeln!(
                output,
                "{}",
                values
                    .iter()
                    .map(|v| text(v))
                    .collect::<Vec<_>>()
                    .join("\t")
            )?;
            continue;
        }
        writeln!(output)?;
        let title = match number {
            Some(number) => format!("{number}. {}", text(&file.name)),
            None => format!("Selected · {}", text(&file.name)),
        };
        for line in textwrap::wrap(&title, width) {
            writeln!(output, "  {}", style.primary(line))?;
        }
        let version = file.version.as_deref().unwrap_or("unknown");
        for value in [
            format!("ID {} · Version {version} · {channel}", file.reference.id.0),
            format!("Minecraft {}", file.game_versions.join(", ")),
            format!(
                "{} · {} bytes · {} dependencies",
                file.filename,
                file.size,
                file.dependencies.len()
            ),
        ] {
            for line in textwrap::wrap(&text(&value), width) {
                writeln!(output, "    {}", style.muted(line))?;
            }
        }
        for loader in &file.loaders {
            writeln!(output, "    {}", style.loader(loader, &text(loader)))?;
        }
        let download = file
            .download_url
            .as_ref()
            .map(|url| url.as_str())
            .unwrap_or("No direct download URL available");
        for line in textwrap::wrap(&text(download), width) {
            writeln!(output, "    {}", style.muted(line))?;
        }
    }
    if formatted {
        writeln!(output)?;
    }
    Ok(())
}
