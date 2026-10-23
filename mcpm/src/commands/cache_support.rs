//! Shared CLI transfer strategies for explicit cache import/export workflows.

use clap::ValueEnum;
use kako_mods_lib::local::mcmod::cache::{ExportMode, ImportMode};

#[derive(Debug, Default, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FileStrategy {
    #[default]
    Copy,
    HardLink,
    PreferHardLink,
}

impl From<FileStrategy> for ExportMode {
    fn from(value: FileStrategy) -> Self {
        match value {
            FileStrategy::Copy => Self::Copy,
            FileStrategy::HardLink => Self::HardLink,
            FileStrategy::PreferHardLink => Self::PreferHardLink,
        }
    }
}

impl From<FileStrategy> for ImportMode {
    fn from(value: FileStrategy) -> Self {
        match value {
            FileStrategy::Copy => Self::Copy,
            FileStrategy::HardLink => Self::HardLink,
            FileStrategy::PreferHardLink => Self::PreferHardLink,
        }
    }
}
