use std::path::PathBuf;

use clap::{Args, ValueEnum};
use kako_mods_lib::local::mc::client::launch::{JavaVersionCheck, LaunchOptions};

#[derive(Clone, Copy, ValueEnum)]
enum VersionCheck {
    Minimum,
    Exact,
    Skip,
}

#[derive(Args)]
#[command(next_help_heading = "Java")]
pub(super) struct RuntimeArgs {
    /// Java executable path or program name resolved through PATH.
    #[arg(long, default_value = "java")]
    pub(super) java: PathBuf,
    /// Compare java -version with the manifest's majorVersion.
    #[arg(long, value_enum, default_value = "minimum")]
    java_version_check: VersionCheck,
    /// Override the host architecture when using a different-architecture JVM.
    #[arg(long)]
    java_arch: Option<String>,
    /// Override JVM bitness for native classifier selection.
    #[arg(long, value_parser = ["32", "64"])]
    java_bits: Option<String>,
    /// Initial heap size in MiB.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    min_memory: Option<u64>,
    /// Maximum heap size in MiB.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    max_memory: Option<u64>,
    /// Thread stack size in KiB.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    stack_size: Option<u64>,
    /// Append one JVM argument; use --jvm-arg=-X... for flags.
    #[arg(long, allow_hyphen_values = true)]
    jvm_arg: Vec<std::ffi::OsString>,
    /// Append one game argument, preserving spaces within the value.
    #[arg(long, allow_hyphen_values = true)]
    game_arg: Vec<std::ffi::OsString>,
    /// Override the resolved main class.
    #[arg(long)]
    main_class: Option<String>,
}

impl RuntimeArgs {
    pub(super) fn apply(&self, options: &mut LaunchOptions) {
        options.java.version_check = match self.java_version_check {
            VersionCheck::Minimum => JavaVersionCheck::AtLeastManifest,
            VersionCheck::Exact => JavaVersionCheck::ExactManifest,
            VersionCheck::Skip => JavaVersionCheck::Skip,
        };
        if let Some(arch) = &self.java_arch {
            let arch = crate::commands::client::platform::normalize_architecture(arch);
            if let Some(bits) = crate::commands::client::platform::architecture_bits(&arch) {
                options.rules.platform.bits = bits;
            }
            options.rules.platform.arch = arch;
        }
        if let Some(bits) = &self.java_bits {
            options.rules.platform.bits = if bits == "32" { 32 } else { 64 };
        }
        options.memory.minimum_mib = self.min_memory;
        options.memory.maximum_mib = self.max_memory;
        options.memory.stack_kib = self.stack_size;
        options.arguments.jvm.append = self.jvm_arg.clone();
        options.arguments.game.append = self.game_arg.clone();
        options.arguments.main_class = self.main_class.clone();
    }
}
