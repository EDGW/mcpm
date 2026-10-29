//! Installed client profiles. Enumeration and metadata inspection never launch the game.

mod assets;
mod check;
mod directory;
pub mod launch;
mod libraries;
mod manifest;
mod path;
mod rules;
mod version;

pub use assets::{AssetError, AssetsDirectory};
pub use check::{
    CheckEvent, CheckIssue, CheckOptionValue, CheckOptions, CheckPlatform, CheckProblem,
    CheckProgress, CheckReport, CheckRules, CheckStep, CheckTarget, CheckedFile, FileCheckStatus,
    LibraryRuleError, PlannedFile,
};
pub use directory::{MinecraftDirectory, MinecraftDirectoryError};
pub use libraries::{LibrariesDirectory, LibraryError, LibraryFile, LibraryFileKind};
pub use manifest::{
    AssetIndex, AssetIndexInfo, AssetObject, ClientLogging, DownloadInfo, JavaRequirement,
    LibraryCoordinateError, LibraryDownloads, LibraryExtract, LibraryRule, LibraryRuleAction,
    LibraryRuleOs, LoggingFile, MavenCoordinate, MinecraftLibrary, MinecraftVersionManifest,
};
pub use path::ResourcePathError;
pub use version::{BrokenVersion, MinecraftVersion, MinecraftVersionError, NormalVersion};

/// Compatibility name for a recognized loader and its declared version.
pub use crate::local::modloader::LoaderInfo as MinecraftModLoader;

pub use rules::{ClientPlatform, RuleEnvironment};
