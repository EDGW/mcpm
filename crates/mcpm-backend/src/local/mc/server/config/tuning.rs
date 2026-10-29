//! JVM setting classification and argument generation without validating Java semantics.

use super::*;
use crate::local::java::command::{self, JavaCommand};
use anyhow::{Result, bail, ensure};

/// Canonical memory/GC identity shared by imports and configuration array classification.
pub(crate) fn managed(argument: &str) -> Option<String> {
    for (prefix, key) in [
        ("-Xms", "InitialHeapSize"),
        ("-Xmx", "MaxHeapSize"),
        ("-Xmn", "YoungHeapSize"),
        ("-Xss", "ThreadStackSize"),
    ] {
        if argument.starts_with(prefix) {
            return Some(key.into());
        }
    }
    if matches!(argument, "-Xincgc" | "-Xnoclassgc") {
        return Some("legacyGC".into());
    }
    let flag = argument
        .strip_prefix("-XX:")?
        .trim_start_matches(['+', '-']);
    let name = flag.split('=').next()?;
    let key = match name {
        "UseSerialGC" | "UseParallelGC" | "UseParallelOldGC" | "UseG1GC" | "UseZGC"
        | "UseShenandoahGC" | "UseEpsilonGC" | "UseConcMarkSweepGC" => {
            return Some(format!("collector:{name}"));
        }
        "NewSize" | "MaxNewSize" => name,
        _ if name.contains("Heap")
            || name.contains("RAM")
            || name.contains("GC")
            || name.contains("Metaspace")
            || name.contains("CodeCache")
            || name.contains("ClassSpace")
            || name.contains("DirectMemory")
            || name.contains("NewRatio")
            || name.contains("Survivor")
            || name.contains("Tenuring")
            || name.starts_with("G1")
            || name.starts_with("Z")
            || name.starts_with("Shenandoah")
            || name.starts_with("CMS")
            || name.starts_with("TLAB")
            || matches!(
                name,
                "ThreadStackSize"
                    | "AlwaysPreTouch"
                    | "UseLargePages"
                    | "UseNUMA"
                    | "UseCompressedOops"
                    | "UseCompressedClassPointers"
                    | "UseStringDeduplication"
                    | "UseAdaptiveSizePolicy"
                    | "UnlockExperimentalVMOptions"
                    | "UnlockDiagnosticVMOptions"
                    | "IgnoreUnrecognizedVMOptions"
                    | "SoftRefLRUPolicyMSPerMB"
            ) =>
        {
            name
        }
        _ => return None,
    };
    Some(key.into())
}

/// Classifies options after consuming their operands with the shared Java grammar.
pub(crate) fn contains_managed(arguments: &[String]) -> Result<bool> {
    let mut probe = arguments.to_vec();
    probe.push("mcpm.validation.Main".into());
    let parsed = JavaCommand::parse(&probe)?;
    Ok(parsed.options().any(|(name, _)| managed(name).is_some()))
}

pub(crate) fn validate(conf: &ServerConfig) -> Result<()> {
    ensure!(
        conf.version == 1,
        "unsupported conf.toml version {}",
        conf.version
    );
    if let Some(executable) = &conf.java.executable {
        nonempty(executable, "java.executable")?;
    }
    ensure!(
        conf.jvm.memory.automatic.heap_fraction.is_none()
            && conf.jvm.memory.automatic.reserve.is_none(),
        "automatic JVM settings are not supported in this version"
    );
    validate_arguments(&conf.application.extra_arguments)?;
    command::validate_jvm(&conf.jvm.memory.extra_arguments)?;
    ensure!(
        conf.jvm
            .memory
            .extra_arguments
            .iter()
            .all(|arg| managed(arg).is_some()),
        "jvm.memory.extraArguments accepts only memory/GC flags"
    );
    command::validate_jvm(&conf.jvm.extra_arguments)?;
    ensure!(
        !contains_managed(&conf.jvm.extra_arguments)?,
        "memory/GC flags belong in jvm.memory.extraArguments"
    );
    command::validate_jvm(&generate(conf)?)?;
    Ok(())
}

pub(crate) fn generate(conf: &ServerConfig) -> Result<Vec<String>> {
    let jvm = &conf.jvm;
    let mut result = Vec::new();
    for (value, flag) in [
        (&jvm.heap.initial, "-Xms"),
        (&jvm.heap.maximum, "-Xmx"),
        (&jvm.heap.young, "-Xmn"),
        (&jvm.heap.maximum_ram, "-XX:MaxRAM="),
        (&jvm.thread_stack, "-Xss"),
        (&jvm.metaspace.initial, "-XX:MetaspaceSize="),
        (&jvm.metaspace.maximum, "-XX:MaxMetaspaceSize="),
        (
            &jvm.metaspace.compressed_class_space,
            "-XX:CompressedClassSpaceSize=",
        ),
        (&jvm.direct.maximum, "-XX:MaxDirectMemorySize="),
        (&jvm.code_cache.initial, "-XX:InitialCodeCacheSize="),
        (&jvm.code_cache.maximum, "-XX:ReservedCodeCacheSize="),
        (&jvm.gc.g1.region_size, "-XX:G1HeapRegionSize="),
    ] {
        if let Some(value) = value {
            ensure!(
                value != "auto",
                "automatic JVM settings are not supported in this version"
            );
            ensure!(!value.contains('\0'), "NUL in JVM setting {flag}");
            result.push(format!("{flag}{value}"));
        }
    }
    for (value, flag) in [
        (jvm.heap.initial_percentage, "InitialRAMPercentage"),
        (jvm.heap.minimum_percentage, "MinRAMPercentage"),
        (jvm.heap.maximum_percentage, "MaxRAMPercentage"),
    ] {
        if let Some(value) = value {
            result.push(format!("-XX:{flag}={value}"));
        }
    }
    if let Some(collector) = jvm.gc.collector {
        result.push(format!("-XX:+{}", collector.flag()?));
    }
    for (value, flag) in [
        (jvm.gc.max_pause_millis, "MaxGCPauseMillis"),
        (jvm.gc.parallel_threads, "ParallelGCThreads"),
        (jvm.gc.concurrent_threads, "ConcGCThreads"),
        (jvm.gc.g1.reserve_percentage, "G1ReservePercent"),
        (
            jvm.gc.g1.initiating_occupancy_percentage,
            "InitiatingHeapOccupancyPercent",
        ),
    ] {
        if let Some(value) = value {
            result.push(format!("-XX:{flag}={value}"));
        }
    }
    for (value, flag) in [
        (jvm.gc.disable_explicit, "DisableExplicitGC"),
        (jvm.gc.explicit_concurrent, "ExplicitGCInvokesConcurrent"),
        (jvm.gc.string_deduplication, "UseStringDeduplication"),
        (jvm.memory.pre_touch, "AlwaysPreTouch"),
        (jvm.memory.large_pages, "UseLargePages"),
    ] {
        if let Some(value) = value {
            result.push(format!("-XX:{}{flag}", if value { '+' } else { '-' }));
        }
    }
    result.extend(jvm.memory.extra_arguments.iter().cloned());
    result.extend(jvm.extra_arguments.iter().cloned());
    Ok(result)
}

impl Collector {
    pub(crate) fn flag(self) -> Result<&'static str> {
        Ok(match self {
            Self::Auto => bail!("automatic GC selection is not supported in this version"),
            Self::Serial => "UseSerialGC",
            Self::Parallel => "UseParallelGC",
            Self::G1 => "UseG1GC",
            Self::Zgc => "UseZGC",
            Self::Shenandoah => "UseShenandoahGC",
            Self::Epsilon => "UseEpsilonGC",
        })
    }
}

pub(crate) fn validate_arguments(arguments: &[String]) -> Result<()> {
    ensure!(
        arguments.iter().all(|value| !value.contains('\0')),
        "NUL in application argument"
    );
    Ok(())
}
