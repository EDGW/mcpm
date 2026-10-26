//! Strict version-one launch and tuning schemas; decoding is owned by the config facade.

pub use crate::local::java::command::JavaEntry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Fixed process structure; loader identity remains exclusively in server metadata.
/// # Implementations
/// - [`Self::validate`], [`Self::encode`]: launch structure validation and encoding in the configuration module.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub platform: Option<Platform>,
    #[serde(default)]
    pub process: Process,
    #[serde(default)]
    pub jvm: Arguments,
    pub entry: JavaEntry,
    #[serde(default)]
    pub application: Arguments,
}

/// Source platform; omission in hand-written configuration means the current platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Unix,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Explicit process environment overrides and server-root-relative working directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Process {
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
}

/// Ordered argv region; each string denotes exactly one argument.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Arguments {
    #[serde(default)]
    pub extra_arguments: Vec<String>,
}

/// User settings. Missing fields never activate defaults or generate tuning flags.
/// # Implementations
/// - [`Self::default`]: empty explicit configuration.
/// - [`Self::validate`], [`Self::jvm_arguments`], [`Self::server_arguments`]: argument generation in the configuration module.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub java: Java,
    #[serde(default)]
    pub jvm: Jvm,
    #[serde(default)]
    pub server: Server,
    #[serde(default)]
    pub application: Arguments,
    #[serde(default, rename = "authlib-injector")]
    pub authlib_injector: Option<super::AuthlibInjector>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            version: 1,
            java: Java::default(),
            jvm: Jvm::default(),
            server: Server::default(),
            application: Arguments::default(),
            authlib_injector: None,
        }
    }
}

/// Java lookup is performed after applying the effective process environment.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Java {
    pub executable: Option<String>,
}

/// JVM tuning with independent ordered memory and general argument regions.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Jvm {
    pub thread_stack: Option<String>,
    #[serde(default)]
    pub heap: Heap,
    #[serde(default)]
    pub metaspace: Metaspace,
    #[serde(default)]
    pub direct: Direct,
    #[serde(default)]
    pub code_cache: CodeCache,
    #[serde(default)]
    pub gc: Gc,
    #[serde(default)]
    pub memory: Memory,
    #[serde(default)]
    pub extra_arguments: Vec<String>,
}

/// Heap sizes and ergonomics percentages; MinRAMPercentage is not an initial heap.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Heap {
    pub initial: Option<String>,
    pub maximum: Option<String>,
    pub young: Option<String>,
    #[serde(rename = "maximumRAM")]
    pub maximum_ram: Option<String>,
    pub initial_percentage: Option<f64>,
    pub minimum_percentage: Option<f64>,
    pub maximum_percentage: Option<f64>,
}

/// Metaspace thresholds and limits, separate from heap memory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metaspace {
    pub initial: Option<String>,
    pub maximum: Option<String>,
    pub compressed_class_space: Option<String>,
}

/// Direct-buffer memory limit.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Direct {
    pub maximum: Option<String>,
}

/// Initial and reserved JIT code-cache sizes.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeCache {
    pub initial: Option<String>,
    pub maximum: Option<String>,
}

/// Explicit collector and collector-dependent tuning. Omission keeps JVM ergonomics.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gc {
    pub collector: Option<Collector>,
    pub max_pause_millis: Option<u32>,
    pub parallel_threads: Option<u32>,
    pub concurrent_threads: Option<u32>,
    pub disable_explicit: Option<bool>,
    pub explicit_concurrent: Option<bool>,
    pub string_deduplication: Option<bool>,
    #[serde(default)]
    pub g1: G1,
}

/// Supported collector names; auto is parsed only to report its explicit unsupported status.
/// # Implementations
/// - [`Self::flag`]: Java flag mapping in `tuning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collector {
    Auto,
    Serial,
    Parallel,
    G1,
    Zgc,
    Shenandoah,
    Epsilon,
}

/// G1 tuning does not implicitly select G1.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct G1 {
    pub region_size: Option<String>,
    pub reserve_percentage: Option<u32>,
    pub initiating_occupancy_percentage: Option<u32>,
}

/// Additional memory/GC flags preserve their own order and never hide in general JVM flags.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Memory {
    pub pre_touch: Option<bool>,
    pub large_pages: Option<bool>,
    #[serde(default)]
    pub extra_arguments: Vec<String>,
    #[serde(default, rename = "auto")]
    pub automatic: AutoMemory,
}

/// Reserved automatic policy input; automatic sizing is not implemented in this version.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoMemory {
    pub heap_fraction: Option<f64>,
    pub reserve: Option<String>,
}

/// Minecraft launch overrides; server.properties is never read or changed here.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub nogui: Option<bool>,
    pub port: Option<u16>,
    pub world: Option<String>,
}

fn version() -> u32 {
    1
}
