//! Lightweight aggregate error, recursive error-tree inspection, and shared error exports.

use std::{error::Error as StdError, fmt};

pub use crate::local::java::executable::JavaExecutableNotFound;

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
        write!(output, "{} independent errors:", self.errors.len())?;
        for (index, error) in self.errors.iter().enumerate() {
            write!(output, "\n  [{}] {error:#}", index + 1)?;
        }
        Ok(())
    }
}

impl StdError for CombinedError {}

/// Tests the complete error tree, walking both `.source()` chains and [`CombinedError`] branches.
pub fn contains<E: StdError + 'static>(error: &(dyn StdError + 'static)) -> bool {
    let mut next = Some(error);
    while let Some(cause) = next {
        if cause.is::<E>() {
            return true;
        }
        if let Some(combined) = cause.downcast_ref::<CombinedError>()
            && combined
                .errors()
                .iter()
                .any(|branch| contains::<E>(branch.as_ref()))
        {
            return true;
        }
        next = cause.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("first leaf")]
    struct FirstLeaf;

    #[derive(Debug, thiserror::Error)]
    #[error("second leaf")]
    struct SecondLeaf;

    #[derive(Debug, thiserror::Error)]
    #[error("absent leaf")]
    struct AbsentLeaf;

    #[test]
    fn contains_walks_nested_combined_branches_and_display_retains_context() {
        let inner = CombinedError::new(
            anyhow::Error::new(FirstLeaf).context("inner context"),
            anyhow::anyhow!("plain branch"),
        );
        let mut outer = CombinedError::new(
            anyhow::Error::new(inner).context("outer branch"),
            anyhow::Error::new(SecondLeaf),
        );
        outer.push(anyhow::anyhow!("third branch"));
        let wrapped = anyhow::Error::new(outer).context("top-level failure");

        assert!(contains::<FirstLeaf>(wrapped.as_ref()));
        assert!(contains::<SecondLeaf>(wrapped.as_ref()));
        assert!(!contains::<AbsentLeaf>(wrapped.as_ref()));

        let rendered = format!("{wrapped:#}");
        assert!(rendered.contains("first leaf"), "{rendered}");
        assert!(rendered.contains("second leaf"), "{rendered}");
        assert!(rendered.contains("third branch"), "{rendered}");
    }
}
