//! Shared aggregate errors and lossless diagnostic snapshots, independent of presentation.

use std::{error::Error as StdError, fmt};

use serde::{Deserialize, Serialize};

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
            message: &mut impl FnMut(&(dyn StdError + 'static)) -> Option<String>,
        ) -> ErrorReport {
            let mut report = ErrorReport::default();
            let mut messages = Vec::new();
            while let Some(cause) = error {
                if let Some(combined) = cause.downcast_ref::<CombinedError>() {
                    report.branches = combined
                        .errors()
                        .iter()
                        .map(|branch| capture(Some(branch.as_ref()), message))
                        .collect();
                    break;
                }
                if !cause.is::<RemoteError>()
                    && let Some(value) = message(cause)
                {
                    messages.push(value);
                }
                error = cause.source();
            }
            if !messages.is_empty() {
                report.message = messages.remove(0);
                report.causes = messages;
            }
            report
        }
        capture(Some(error), &mut message)
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
        for context in std::iter::once(self.message).chain(self.causes).rev() {
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
}
