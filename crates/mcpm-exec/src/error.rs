//! Shared aggregate errors, remote failure identity and lossless diagnostic snapshots.
#![doc = include_str!("docs/error.md")]

use std::{error::Error as StdError, fmt};

use serde::{Deserialize, Serialize};

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

/// Identifies executable lookup failure without relying on localized OS error text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[error("executable {executable:?} was not found")]
pub struct ExecutableNotFound {
    pub executable: String,
    /// Startup discovery can establish remote origin before an operation session exists.
    pub remote: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_not_found: Option<ExecutableNotFound>,
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
                if let Some(missing) = cause.downcast_ref::<ExecutableNotFound>() {
                    report.executable_not_found = Some(missing.clone());
                }
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
        let mut missing = self.executable_not_found;
        let missing_message = missing.as_ref().map(ToString::to_string);
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
            let typed = if missing_message.as_deref() == Some(context.as_str()) {
                missing.take()
            } else {
                None
            };
            error = Some(match (error, typed) {
                (Some(error), Some(missing)) => error.context(missing),
                (None, Some(missing)) => anyhow::Error::new(missing),
                (Some(error), None) => error.context(context),
                (None, None) => anyhow::Error::msg(context),
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

    #[test]
    fn executable_identity_survives_transport_without_classifying_missing_data_files() {
        let missing = ExecutableNotFound {
            executable: "java".into(),
            remote: false,
        };
        let original = anyhow::Error::new(missing.clone()).context("prepare Java");
        let report = ErrorReport::capture(&original);
        assert_eq!(report.executable_not_found, Some(missing));
        let encoded = serde_json::to_vec(&report).unwrap();
        let restored = serde_json::from_slice::<ErrorReport>(&encoded)
            .unwrap()
            .into_error();
        let remote = RemoteError::wrap(restored);
        assert_eq!(ErrorReport::capture(&remote), report);
        let data_file = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(
            ErrorReport::capture(&data_file)
                .executable_not_found
                .is_none()
        );
    }
}
