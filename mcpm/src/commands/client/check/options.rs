//! CLI verification modes and resource-specific check options.

use clap::{Args, ValueEnum};
use mcpm_backend::local::mc::client::{CheckOptionValue, CheckOptions};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Size,
    Hash,
    Skip,
}

impl From<Mode> for CheckOptionValue {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Size => Self::Size,
            Mode::Hash => Self::Hash,
            Mode::Skip => Self::Skip,
        }
    }
}

#[derive(Args, Debug)]
pub(super) struct OptionArgs {
    /// Verify every category using size and SHA-1; conflicts with individual policies.
    #[arg(long, conflicts_with_all = ["shallow", "libraries", "assets", "asset_index", "version_jar", "logging"])]
    strict: bool,
    /// Verify every category using size only; conflicts with individual policies.
    #[arg(long, conflicts_with_all = ["libraries", "assets", "asset_index", "version_jar", "logging"])]
    shallow: bool,
    /// Library/native verification: size, size then SHA-1, or skip.
    #[arg(long, value_enum, default_value = "hash")]
    libraries: Mode,
    /// Asset object verification: size, size then SHA-1, or skip.
    #[arg(long, value_enum, default_value = "size")]
    assets: Mode,
    /// Asset index verification; skipped indexes are still read when assets need them.
    #[arg(long, value_enum, default_value = "hash")]
    asset_index: Mode,
    /// Client JAR verification, respecting jar aliases and inherited base versions.
    #[arg(long, value_enum, default_value = "hash")]
    version_jar: Mode,
    /// Verify the declared client logging configuration: size, hash, or skip.
    #[arg(long, value_enum, default_value = "hash")]
    logging: Mode,
}

impl OptionArgs {
    pub(super) fn selected(&self) -> CheckOptions {
        let preset = if self.strict {
            Some(CheckOptionValue::Hash)
        } else if self.shallow {
            Some(CheckOptionValue::Size)
        } else {
            None
        };
        if let Some(mode) = preset {
            return CheckOptions {
                libraries: mode,
                assets: mode,
                asset_index: mode,
                version_jar: mode,
                logging: mode,
            };
        }
        CheckOptions {
            libraries: self.libraries.into(),
            assets: self.assets.into(),
            asset_index: self.asset_index.into(),
            version_jar: self.version_jar.into(),
            logging: self.logging.into(),
        }
    }
}
