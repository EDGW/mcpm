//! Console log formatting with local crate labels and italic severity for forwarded events.

use std::fmt;

use owo_colors::{OwoColorize, Style};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{
        FmtContext, FormatEvent, FormatFields,
        format::{Compact, Format, Writer},
    },
    registry::LookupSpan,
};

#[derive(Debug)]
pub(super) struct Console {
    local: Format<Compact, ()>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            local: tracing_subscriber::fmt::format()
                .compact()
                .without_time()
                .with_target(false),
        }
    }
}

impl<S, N> FormatEvent<S, N> for Console
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        if event.metadata().target() != super::REMOTE_TARGET {
            let metadata = event.metadata();
            let source = metadata.module_path().unwrap_or(metadata.target());
            let crate_name = source.split_once("::").map_or(source, |(name, _)| name);
            write!(writer, "[")?;
            for (index, part) in crate_name.split('_').enumerate() {
                if index != 0 {
                    write!(writer, "-")?;
                }
                write!(writer, "{part}")?;
            }
            write!(writer, "] ")?;
            return self.local.format_event(ctx, writer, event);
        }
        let level = event.metadata().level();
        if writer.has_ansi_escapes() {
            let style = match *level {
                Level::ERROR => Style::new().red(),
                Level::WARN => Style::new().yellow(),
                Level::INFO => Style::new().green(),
                Level::DEBUG => Style::new().blue(),
                Level::TRACE => Style::new().purple(),
            }
            .italic();
            write!(writer, "{:>5} ", level.as_str().style(style))?;
        } else {
            write!(writer, "{level:>5} ")?;
        }
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}
