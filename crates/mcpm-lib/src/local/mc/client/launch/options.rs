//! Typed client launch options.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

use super::{EnvName, Identity};
use crate::local::mc::client::{CheckOptions, RuleEnvironment};

/// Required inputs are explicit; remaining groups have conservative defaults.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub java: JavaOptions,
    pub identity: Identity,
    /// Describes the selected JVM's platform/architecture, not necessarily the host CPU.
    pub rules: RuleEnvironment,
    pub directories: DirectoryOptions,
    pub arguments: ArgumentOptions,
    pub memory: MemoryOptions,
    pub window: WindowOptions,
    pub launcher: LauncherInfo,
    pub logging: LoggingOptions,
    pub natives: NativeOptions,
    pub process: ProcessOptions,
    pub quick_play: Option<QuickPlay>,
    pub server: Option<ServerOptions>,
    /// Optional resource validation during prepare. Planning never hashes files.
    pub verification: Option<CheckOptions>,
}

impl LaunchOptions {
    pub fn new(java: impl Into<PathBuf>, identity: Identity, rules: RuleEnvironment) -> Self {
        Self {
            java: JavaOptions {
                executable: java.into(),
                version_check: JavaVersionCheck::default(),
            },
            identity,
            rules,
            directories: DirectoryOptions::default(),
            arguments: ArgumentOptions::default(),
            memory: MemoryOptions::default(),
            window: WindowOptions::default(),
            launcher: LauncherInfo::default(),
            logging: LoggingOptions::default(),
            natives: NativeOptions::default(),
            process: ProcessOptions::default(),
            quick_play: None,
            server: None,
            verification: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JavaOptions {
    /// Absolute path, relative path, or a program name searched on PATH.
    pub executable: PathBuf,
    pub version_check: JavaVersionCheck,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JavaVersionCheck {
    Skip,
    /// Verify `java -version` against javaVersion.majorVersion when declared.
    #[default]
    AtLeastManifest,
    ExactManifest,
}

#[derive(Debug, Clone, Default)]
pub struct DirectoryOptions {
    /// Defaults to the Minecraft root. Set to version.path() for version isolation.
    pub game: Option<PathBuf>,
    /// Defaults to game; affects the process working directory only.
    pub working: Option<PathBuf>,
    /// Override shared libraries/assets caches without moving the Minecraft root.
    pub libraries: Option<PathBuf>,
    pub assets: Option<PathBuf>,
    pub client_jar: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ArgumentOptions {
    pub main_class: Option<String>,
    pub jvm: ArgumentList,
    pub game: ArgumentList,
    /// JVM -D properties added after manifest JVM arguments.
    pub system_properties: BTreeMap<String, OsString>,
    pub classpath_prepend: Vec<PathBuf>,
    pub classpath_append: Vec<PathBuf>,
    /// Extra placeholder values. Reserved built-ins cannot be overwritten.
    pub variables: BTreeMap<String, OsString>,
    pub unknown_variables: UnknownVariablePolicy,
    pub version_name: Option<String>,
    pub version_type: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ArgumentList {
    /// None uses resolved manifest arguments; Some replaces them (even when empty).
    pub replace: Option<Vec<OsString>>,
    pub prepend: Vec<OsString>,
    pub append: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnknownVariablePolicy {
    #[default]
    Error,
    Preserve,
}

/// No implicit heap size or collector selection; Java interprets the emitted sizes.
#[derive(Debug, Clone, Default)]
pub struct MemoryOptions {
    pub minimum_mib: Option<u64>,
    pub maximum_mib: Option<u64>,
    pub stack_kib: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct WindowOptions {
    /// Width and height must be supplied together.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fullscreen: bool,
    pub demo: bool,
}

#[derive(Debug, Clone)]
pub struct LauncherInfo {
    pub name: String,
    pub version: String,
}

impl Default for LauncherInfo {
    fn default() -> Self {
        Self {
            name: "mcpm".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum LoggingOptions {
    /// Use the manifest's local assets/log_configs/<id> file when declared.
    #[default]
    Manifest,
    Disabled,
    Custom {
        file: PathBuf,
        argument: String,
    },
}

#[derive(Debug, Clone)]
pub struct NativeOptions {
    /// None uses versions/<version>/<version>-natives.
    /// Prepared files are retained in both default and custom directories.
    pub directory: Option<PathBuf>,
    pub extract: bool,
    /// Maximum total uncompressed bytes extracted across selected archives.
    pub maximum_bytes: u64,
}

impl Default for NativeOptions {
    fn default() -> Self {
        Self {
            directory: None,
            extract: true,
            maximum_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProcessStream {
    #[default]
    Inherit,
    Null,
    Piped,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessOptions {
    /// Preserve the parent environment unless explicitly requested otherwise.
    pub clear_environment: bool,
    /// Some(value) sets a variable; None removes it.
    pub environment: BTreeMap<EnvName, Option<OsString>>,
    pub stdin: ProcessStream,
    pub stdout: ProcessStream,
    pub stderr: ProcessStream,
}

#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub address: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct QuickPlay {
    pub target: QuickPlayTarget,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum QuickPlayTarget {
    Singleplayer(String),
    Multiplayer(String),
    Realms(String),
}
