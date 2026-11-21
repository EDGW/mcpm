//! Shared aggregate errors, remote failure identity and lossless diagnostic snapshots.
#![doc = include_str!("../../docs/error.md")]

use std::{error::Error as StdError, fmt};

use serde::{Deserialize, Serialize};

// Re-exported Modules

mod tip;
pub use tip::*;

/// An error carried inside operation data; local execution keeps the original error.
///
/// Serialization uses [`ErrorReport`]; deserialization restores the error tree under a
/// [`RemoteError`] marker. Concrete error types are preserved only without serialization.
#[derive(Debug)]
pub struct PortableError(anyhow::Error);

impl PortableError {
    /// Borrow the original local error or its restored remote error tree.
    pub fn as_inner(&self) -> &anyhow::Error {
        &self.0
    }

    /// Returns the original local error or the restored, marked remote error.
    pub fn into_inner(self) -> anyhow::Error {
        self.0
    }
}

impl From<anyhow::Error> for PortableError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl Serialize for PortableError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ErrorReport::capture(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PortableError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let report = ErrorReport::deserialize(deserializer)?;
        Ok(Self(RemoteError::wrap(report.into_error())))
    }
}

/// Independent failures in their original order; branches may contain further aggregates.
#[derive(Debug)]
pub struct CombinedError {
    errors: Vec<anyhow::Error>,
}

impl CombinedError {
    /// Requires two failures; additional independent failures can be appended.
    pub fn new(first: anyhow::Error, second: anyhow::Error) -> Self {
        Self {
            errors: vec![first, second],
        }
    }

    pub fn push(&mut self, error: anyhow::Error) {
        self.errors.push(error);
    }

    pub fn errors(&self) -> &[anyhow::Error] {
        &self.errors
    }
}

impl fmt::Display for CombinedError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{} independent errors", self.errors.len())
    }
}

impl StdError for CombinedError {}

/// Marks a failure received from another process while preserving its original sources.
#[derive(Debug, thiserror::Error)]
#[error("remote operation failed")]
pub struct RemoteError(#[source] anyhow::Error);

impl RemoteError {
    pub fn wrap(error: anyhow::Error) -> anyhow::Error {
        Self(error).into()
    }
}

/// Tests the complete error tree, including all independent branches.
pub fn contains<T: StdError + 'static>(error: &(dyn StdError + 'static)) -> bool {
    let mut next = Some(error);
    while let Some(cause) = next {
        if cause.is::<T>() {
            return true;
        }
        if let Some(combined) = cause.downcast_ref::<CombinedError>()
            && combined
                .errors()
                .iter()
                .any(|branch| contains::<T>(branch.as_ref()))
        {
            return true;
        }
        next = cause.source();
    }
    false
}

/// A linear context chain ending in zero or more independent diagnostic branches.
/// Empty messages are reserved for transparent aggregate roots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorReport {
    pub message: String,
    #[serde(default)]
    pub causes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<ErrorReport>,
    /// Tips of [`TippedError`] nodes in this chain, outermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tips: Vec<ReportTip>,
}

impl ErrorReport {
    pub fn capture(error: &anyhow::Error) -> Self {
        Self::capture_std(error.as_ref())
    }

    pub fn capture_std(error: &(dyn StdError + 'static)) -> Self {
        Self::capture_with(error, |cause| Some(cause.to_string()))
    }

    /// Maps messages while retaining structure; None skips a transparent wrapper.
    pub fn capture_with(
        error: &(dyn StdError + 'static),
        mut message: impl FnMut(&(dyn StdError + 'static)) -> Option<String>,
    ) -> Self {
        fn capture(
            mut error: Option<&(dyn StdError + 'static)>,
            mut remote: bool,
            message: &mut impl FnMut(&(dyn StdError + 'static)) -> Option<String>,
        ) -> ErrorReport {
            let mut report = ErrorReport::default();
            let mut messages = Vec::new();
            while let Some(cause) = error {
                error = cause.source();
                if cause.is::<RemoteError>() {
                    remote = true;
                    continue;
                }
                if let Some(tipped) = cause.downcast_ref::<TippedError>() {
                    report.tips.push(if remote {
                        ReportTip {
                            local: tipped.remote().cloned(),
                            remote: tipped.remote().cloned(),
                        }
                    } else {
                        ReportTip {
                            local: tipped.local().cloned(),
                            remote: tipped.remote().cloned(),
                        }
                    });
                    continue;
                }
                if let Some(combined) = cause.downcast_ref::<CombinedError>() {
                    report.branches = combined
                        .errors()
                        .iter()
                        .map(|branch| capture(Some(branch.as_ref()), remote, message))
                        .collect();
                    break;
                }
                if let Some(value) = message(cause) {
                    messages.push(value);
                }
            }
            if !messages.is_empty() {
                report.message = messages.remove(0);
                report.causes = messages;
            }
            report
        }
        capture(Some(error), false, &mut message)
    }

    pub fn into_error(self) -> anyhow::Error {
        let mut branches = self.branches.into_iter().map(Self::into_error);
        let mut error = branches.next().map(|first| {
            if let Some(second) = branches.next() {
                let mut combined = CombinedError::new(first, second);
                for branch in branches {
                    combined.push(branch);
                }
                anyhow::Error::new(combined)
            } else {
                first
            }
        });
        let mut contexts = std::iter::once(self.message).chain(self.causes).rev();
        if error.is_none() && !self.tips.is_empty() {
            // Tips need a source; the innermost message provides it, as before capture.
            error = contexts
                .find(|context| !context.is_empty())
                .map(anyhow::Error::msg);
        }
        if let Some(source) = error.take() {
            error = Some(self.tips.into_iter().rev().fold(source, |source, tip| {
                TippedError::new(source, tip.local, tip.remote).into()
            }));
        }
        for context in contexts {
            if context.is_empty() && error.is_some() {
                continue;
            }
            error = Some(match error {
                Some(error) => error.context(context),
                None => anyhow::Error::msg(context),
            });
        }
        error.unwrap_or_else(|| anyhow::anyhow!("unknown error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_rich::rich;

    #[test]
    fn nested_branches_survive_capture_serialization_and_restoration() {
        let inner = CombinedError::new(
            anyhow::anyhow!("Error7")
                .context("Error6")
                .context("Error5"),
            anyhow::anyhow!("Error8"),
        );
        let outer = CombinedError::new(
            anyhow::anyhow!("Error3").context("Error2"),
            anyhow::Error::new(inner).context("Error4"),
        );
        let error = anyhow::Error::new(outer)
            .context("Error1")
            .context("Error0");
        let report = ErrorReport::capture(&error);
        assert_eq!(report.causes, ["Error1"]);
        assert_eq!(report.branches[0].causes, ["Error3"]);
        assert_eq!(report.branches[1].branches[0].causes, ["Error6", "Error7"]);
        let bytes = serde_json::to_vec(&report).unwrap();
        let restored: ErrorReport = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(ErrorReport::capture(&restored.into_error()), report);
        let old: ErrorReport =
            serde_json::from_str(r#"{"message":"outer","causes":["inner"]}"#).unwrap();
        assert!(old.branches.is_empty());
        assert_eq!(ErrorReport::capture(&old.clone().into_error()), old);
    }

    #[test]
    fn guidance_survives_nested_transport_without_becoming_a_failure_cause() {
        let upgrade = rich!("Upgrade the file.");
        let retry = rich!("Retry after resolving both failures.");
        let failures = anyhow::Error::new(CombinedError::new(
            Err::<(), _>(anyhow::anyhow!("unsupported format"))
                .tip(upgrade.clone())
                .unwrap_err(),
            anyhow::anyhow!("independent I/O error"),
        ));
        let original = Err::<(), _>(failures)
            .tip(retry.clone())
            .unwrap_err()
            .context("operation failed");
        let report = ErrorReport::capture(&original);
        assert_eq!(report.message, "operation failed");
        assert!(report.causes.is_empty());
        assert_eq!(report.tips.len(), 1);
        assert_eq!(report.tips[0].local, Some(retry));
        assert_eq!(report.branches[0].message, "unsupported format");
        assert!(report.branches[0].causes.is_empty());
        assert_eq!(report.branches[0].tips[0].local, Some(upgrade));
        let bytes = serde_json::to_vec(&report).unwrap();
        let decoded: ErrorReport = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(ErrorReport::capture(&decoded.into_error()), report);
    }

    #[test]
    fn remote_tip_becomes_final_once_a_remote_boundary_is_crossed() {
        fn received(report: &ErrorReport) -> ErrorReport {
            let bytes = serde_json::to_vec(report).unwrap();
            let decoded: ErrorReport = serde_json::from_slice(&bytes).unwrap();
            ErrorReport::capture(&RemoteError::wrap(decoded.into_error()))
        }
        let local = rich!("Local advice.");
        let remote = rich!("Remote advice.");
        let original = Err::<(), _>(anyhow::anyhow!("inner"))
            .tips(Some(local.clone()), Some(remote.clone()))
            .unwrap_err()
            .context("outer");
        let here = ErrorReport::capture(&original);
        assert_eq!(here.tips[0].local, Some(local));
        assert_eq!(here.causes, ["inner"]);
        let crossed = received(&here);
        assert_eq!(crossed.tips[0].local, Some(remote));
        assert_eq!(
            (&crossed.message, &crossed.causes),
            (&here.message, &here.causes)
        );
        assert_eq!(received(&crossed), crossed);

        let missing = ErrorReport::capture(&TippedError::executable_not_found("git").into());
        assert_eq!(missing.tips[0].local, None);
        assert!(received(&missing).tips[0].local.is_some());
    }
}
