//! Shared source-selection arguments; provider specifications remain on the client.

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub(in crate::commands) struct SourcesArgs {
    /// Comma-separated source list; explicit selections replace the default pair.
    #[arg(long, value_name = "SOURCES", value_delimiter = ',')]
    sources: Option<Vec<String>>,
    /// Select a source; repeat or combine with --sources. Accepts curseforge@KEY.
    #[arg(long, value_name = "SOURCE", value_delimiter = ',')]
    source: Vec<String>,
}

impl SourcesArgs {
    pub fn selected(&self) -> Result<Vec<String>> {
        mcpm_backend::source::provider::select(self.sources.clone(), self.source.clone())
    }
}

impl std::fmt::Debug for SourcesArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.selected()
                    .ok()
                    .into_iter()
                    .flatten()
                    .map(|s| s.split('@').next().unwrap_or(&s).to_owned()),
            )
            .finish()
    }
}
