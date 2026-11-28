//! Crash report analysis operation on the file-owning host.

use anyhow::Result;
use mcpm_exec::operation::Operation;
pub use mcpm_lib::local::mc::crash_report::{CrashReportAnalysis, CrashReportType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Analyses a Minecraft crash report file on the target Host.
pub struct Analyse;
impl Operation for Analyse {
    const ID: &'static str = "crashreport.analyse";
    type Input = AnalyseInput;
    type Output = CrashReportAnalysis;

    async fn run(input: AnalyseInput) -> Result<CrashReportAnalysis> {
        tokio::task::spawn_blocking(move || {
            Ok(mcpm_lib::local::mc::crash_report::analyse_file(
                &input.path,
            )?)
        })
        .await?
    }
}

/// Input for [`Analyse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyseInput {
    pub path: PathBuf,
}
