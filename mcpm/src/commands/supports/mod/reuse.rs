//! Shared CLI cache selection; all directory and identification behavior belongs to the library.

use std::path::PathBuf;

use clap::Args;
use mcpm_backend::local::{cache::mods::ModsCacheSelection, runtime::RuntimeEnvironment};

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
    pub fn environment(self) -> RuntimeEnvironment {
        RuntimeEnvironment::new().with_mods_cache(self.selection())
    }

    pub fn selection(self) -> Option<ModsCacheSelection> {
        if self.no_cache {
            None
        } else {
            Some(match self.cache_dir {
                Some(path) => ModsCacheSelection::Directory { path },
                None => ModsCacheSelection::Default,
            })
        }
    }
}

/// Per-operation query policy shared with Server commands; it never selects a directory.
#[derive(Debug, Args, Default)]
pub(in crate::commands) struct QueryArgs {
    /// Identify files directly without querying the configured Mod cache.
    #[arg(long, help_heading = "Loading options")]
    pub no_cache: bool,
}

impl QueryArgs {
    pub fn environment(&self, environment: &RuntimeEnvironment) -> RuntimeEnvironment {
        if self.no_cache {
            environment.clone().with_mods_cache(None)
        } else {
            environment.clone()
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
            crate::display::style::OutputStyle::new().muted(summary_line(summary))
        )?;
    }
    Ok(())
}

/// A host-resolved cache target for subsequent remote queries or source publication.
fn resolved_selection(
    environment: &RuntimeEnvironment,
) -> anyhow::Result<Option<ModsCacheSelection>> {
    Ok(environment
        .mods_cache()?
        .map(|cache| ModsCacheSelection::Directory {
            path: cache.path().to_owned(),
        }))
}

/// Transport configuration is extracted without opening a remote cache locally.
pub(in crate::commands) fn selection(
    environment: &RuntimeEnvironment,
) -> Option<ModsCacheSelection> {
    environment
        .cache
        .mods
        .as_ref()
        .map(|cache| cache.selection().clone())
}

/// Cache configuration resolved on the file host; contains no local resource handles.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct HostEnvironment {
    pub cache: Option<ModsCacheSelection>,
}

impl HostEnvironment {
    pub fn capture(environment: &RuntimeEnvironment) -> anyhow::Result<Self> {
        Ok(Self {
            cache: resolved_selection(environment)?,
        })
    }

    pub fn into_runtime(self) -> RuntimeEnvironment {
        RuntimeEnvironment::new().with_mods_cache(self.cache)
    }
}
