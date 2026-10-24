//! Shared CLI cache selection; all directory and identification behavior belongs to the library.

use std::path::PathBuf;

use clap::Args;
use kako_mods_lib::local::mcmod::ModOpenCache;

#[derive(Debug, Args)]
pub(in crate::commands) struct CacheArgs {
    /// Disable the default read-only cache and identify files directly.
    #[arg(long, conflicts_with = "cache_dir", help_heading = "Loading options")]
    no_cache: bool,
    /// Use this initialized cache instead of ~/.mcpm/cache/mods on the file host.
    #[arg(long, value_name = "PATH", help_heading = "Loading options")]
    cache_dir: Option<PathBuf>,
}

impl CacheArgs {
    pub fn selection(self) -> Option<ModOpenCache> {
        if self.no_cache {
            None
        } else {
            Some(match self.cache_dir {
                Some(path) => ModOpenCache::Directory { path },
                None => ModOpenCache::Default,
            })
        }
    }
}

/// Common summary text for Console and TUI; Raw output uses structured completion data.
pub(in crate::commands) fn summary_line(summary: super::scan::CacheSummary) -> String {
    format!(
        "{} fully cached · {} partially cached · {} total items",
        summary.fully_cached, summary.partially_cached, summary.total
    )
}

pub(in crate::commands) fn write_summary(
    output: &mut impl std::io::Write,
    summary: super::scan::CacheSummary,
) -> std::io::Result<()> {
    if crate::terminal::stdout_is_tty() {
        writeln!(output)?;
        writeln!(
            output,
            "{}",
            crate::commands::output_style::OutputStyle::new().muted(summary_line(summary))
        )?;
    }
    Ok(())
}
