//! Client logging configuration handling.

use super::{CheckIssue, CheckProblem, CheckReport, CheckStep, CheckTarget, PlannedFile};
use crate::local::mc::client::{MinecraftDirectory, MinecraftVersionManifest};

pub(super) fn plan(
    directory: &MinecraftDirectory,
    manifest: &MinecraftVersionManifest,
    report: &mut CheckReport,
) -> Option<PlannedFile> {
    // Logging is optional. Check exactly the effective declaration used at launch.
    let logging = manifest.client_logging()?;
    let target = CheckTarget::Logging {
        id: logging.file.id.clone(),
    };
    match directory.assets().log_config_path(&logging.file.id) {
        Ok(path) => Some(PlannedFile {
            step: CheckStep::Logging,
            target,
            path,
            size: logging.file.size,
            checksums: logging.file.sha1.clone().into_iter().collect(),
            mode: report.options.logging,
        }),
        Err(source) => {
            report.issues.push(CheckIssue {
                target,
                path: None,
                problem: CheckProblem::Path { source },
            });
            None
        }
    }
}
