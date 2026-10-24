//! Shared CLI transfer strategies for explicit cache import/export workflows.

use clap::ValueEnum;
use mcpm_backend::local::cache::CacheLinkPreference;

#[derive(Debug, Default, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FileStrategy {
    #[default]
    Copy,
    HardLink,
    PreferHardLink,
}

impl From<FileStrategy> for CacheLinkPreference {
    fn from(value: FileStrategy) -> Self {
        match value {
            FileStrategy::Copy => Self::Copy,
            FileStrategy::HardLink => Self::HardLink,
            FileStrategy::PreferHardLink => Self::PreferHardLink,
        }
    }
}
