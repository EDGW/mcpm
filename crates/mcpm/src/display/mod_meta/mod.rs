//! Common declaration rendering for runtime ModMeta and portable CachedModMeta data.

use std::{
    io::{self, Write},
    path::Path,
};

use super::{
    console::ConsoleWriter,
    dependency::format_dependency,
    style::{OutputStyle, text},
};

mod view;
use view::Content;
pub(crate) use view::{Declaration, View};

pub(crate) fn write<'a>(
    out: &mut ConsoleWriter<'_>,
    path: &Path,
    metadata: impl Into<View<'a>>,
) -> io::Result<()> {
    let metadata = metadata.into();
    if matches!(&metadata.content, Content::Missing) {
        return Ok(());
    }
    let style = OutputStyle::new();
    writeln!(
        out,
        "\n{}{}",
        style.muted("Loader: "),
        style.loader(metadata.loader, &text(metadata.loader_name))
    )?;
    let mut out = out.subwriter();
    let Content::Recognized(records) = &metadata.content else {
        writeln!(out, "{}", style.muted("Identification: not recognized"))?;
        return Ok(());
    };
    for (index, item) in records.iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        let info = item.info;
        writeln!(
            out,
            "{}{}",
            style.muted("Name: "),
            style.primary(text(&info.name))
        )?;
        writeln!(
            out,
            "{}{}",
            style.muted("ID: "),
            style.important(text(&info.modid))
        )?;
        writeln!(
            out,
            "{}{}",
            style.muted("Version: "),
            style.secondary(text(&info.version))
        )?;
        let source = std::iter::once(text(&path.to_string_lossy()))
            .chain(item.source.iter().map(|part| text(part)))
            .map(|part| style.muted(part))
            .collect::<Vec<_>>()
            .join(&style.important("!"));
        writeln!(out, "{}{source}", style.muted("Source: "))?;
        if info.dependencies.is_empty() {
            writeln!(out, "{}", style.muted("Dependencies: none"))?;
        } else {
            writeln!(out, "{}", style.important("Dependencies:"))?;
            let mut out = out.subwriter();
            for dependency in &info.dependencies {
                writeln!(out, "{}", format_dependency(dependency, &style))?;
            }
        }
    }
    Ok(())
}
