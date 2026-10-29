//! Console log formatting with italic severity labels for forwarded remote events.

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
