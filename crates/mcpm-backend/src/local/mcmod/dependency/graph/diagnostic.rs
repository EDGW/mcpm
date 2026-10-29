//! Diagnostic records retained by dependency graphs and their snapshots.

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ErrorReport;

/// A skipped identification failure, retained as snapshot diagnostics.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentificationFailure {
    pub path: PathBuf,
    pub loader: String,
    /// Context and cause chain supplied by the loader.
    #[serde(deserialize_with = "diagnostic")]
    pub error: ErrorReport,
}

/// Version-one snapshots stored a flattened string. Treat it as one opaque message.
fn diagnostic<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ErrorReport, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Structured(ErrorReport),
        Legacy(String),
    }
    Ok(match Stored::deserialize(deserializer)? {
        Stored::Structured(report) => report,
        Stored::Legacy(message) => ErrorReport {
            message,
            ..ErrorReport::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CombinedError;

    #[test]
    fn diagnostics_roundtrip_branches_and_read_legacy_strings_as_opaque_messages() {
        let error = anyhow::Error::new(CombinedError::new(
            anyhow::anyhow!("inner").context("first"),
            anyhow::anyhow!("second"),
        ))
        .context("outer");
        let failure = IdentificationFailure {
            path: "broken.jar".into(),
            loader: "fabric".into(),
            error: ErrorReport::capture(&error),
        };
        let bytes = serde_json::to_vec(&failure).unwrap();
        let restored: IdentificationFailure = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.error, failure.error);
        let old: IdentificationFailure = serde_json::from_str(
            r#"{"path":"broken.jar","loader":"fabric","error":"outer: inner"}"#,
        )
        .unwrap();
        assert_eq!(old.error.message, "outer: inner");
        assert!(old.error.causes.is_empty());
    }
}
