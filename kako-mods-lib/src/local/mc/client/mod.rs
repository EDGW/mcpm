//! Installed client profiles. Enumeration and metadata inspection never launch the game.

use crate::local::modloader::ModLoaderMeta;

mod assets;
mod check;
mod directory;
mod libraries;
mod manifest;
mod path;
mod version;

pub use assets::{AssetError, AssetsDirectory};
pub use check::{
    CheckEvent, CheckIssue, CheckOptionValue, CheckOptions, CheckPlatform, CheckProblem,
    CheckProgress, CheckReport, CheckRules, CheckStep, CheckTarget, CheckedFile, FileCheckStatus,
    LibraryRuleError, PlannedFile,
};
pub use directory::{MinecraftDirectory, MinecraftDirectoryError};
pub use libraries::{LibrariesDirectory, LibraryError, LibraryFile};
pub use manifest::{
    AssetIndex, AssetIndexInfo, AssetObject, DownloadInfo, LibraryCoordinateError,
    LibraryDownloads, LibraryExtract, LibraryRule, LibraryRuleAction, LibraryRuleOs,
    MavenCoordinate, MinecraftLibrary, MinecraftVersionManifest,
};
pub use path::ResourcePathError;
pub use version::{BrokenVersion, MinecraftVersion, MinecraftVersionError, NormalVersion};

/// One recognized loader and its declared version, retained as a string.
#[derive(Debug, Clone)]
pub struct MinecraftModLoader {
    pub loader: ModLoaderMeta,
    pub version: String,
}
