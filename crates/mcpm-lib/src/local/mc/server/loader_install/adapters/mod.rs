//! Loader installer adapter dispatch, allowlist publish contract, and profile path helpers.
//!
//! Adapters share an internal execution and publish description rather than a single
//! upstream profile schema: [`fml`] covers Forge (legacy, transitional, modern) and NeoForge
//! via `install_profile.json`, while [`fabric`] covers Fabric Installer with the Loader 0.12.0
//! property/properties boundary.

use std::{
    collections::BTreeMap,
    path::{Component, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{preflight::PreflightSnapshot, runner::StagingDir};
use crate::download::loader::LoaderFamily;

mod fabric;
mod fml;

/// Adapter-declared allowlist of files to publish from `stage/<workingDirectory>/` to `<server>/`.
///
/// Using an allowlist ensures `run.sh`, `run.bat`, `user_jvm_args.txt`, `*_args.txt`,
/// installer logs, `reports/`, Forge 1.5.2's pre-placed `minecraft_server.1.5.2.jar`,
/// and Fabric's verification vanilla copy are naturally excluded without maintaining a denylist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishPlan {
    /// Runtime files relative to `stage/<workingDirectory>/` (typically 1–3 top-level files).
    pub explicit_files: Vec<PathBuf>,
    /// Pre-existing files intentionally merged in-place by an adapter (e.g. legacy Fabric
    /// `fabric-server-launcher.properties`), mapped to the BLAKE3 digest read during staging.
    /// Publication re-verifies this digest under the write lock before replacing the file.
    pub merged_files: BTreeMap<PathBuf, String>,
    /// Collect all regular files under `stage/<workingDirectory>/libraries/`, skipping
    /// `unix_args.txt` and `win_args.txt` (already expanded into `launch.toml`).
    pub include_libraries: bool,
}

/// Pre-installer preparation returned by an adapter before invoking Java.
#[derive(Debug)]
pub(crate) struct PreparedInstall {
    /// Arguments passed after `-jar <abs/installer.jar>`.
    pub installer_args: Vec<String>,
    /// Adapter-specific post-install continuation state.
    plan: AdapterPlan,
}

#[derive(Debug)]
enum AdapterPlan {
    Fml(fml::FmlPlan),
    Fabric(fabric::FabricPlan),
}

/// Post-installer output consumed by [`super::verify`] and [`super::publish`].
#[derive(Debug)]
pub(crate) struct InstalledArtifacts {
    /// Initial argv passed to [`crate::local::java::command::expand`] in `stage_cwd`.
    pub initial_argv: Vec<String>,
    /// True when a platform-specific `@argfile` (`unix_args.txt` / `win_args.txt`) is expanded,
    /// requiring `LaunchConfig.platform = Some(Platform::current())`.
    pub expanded_platform_argfile: bool,
    /// Allowlist of staged runtime artifacts to publish into the real server root.
    pub publish_plan: PublishPlan,
    /// Set for Fabric `>= 0.12.0` so `verify` rejects conflicting `-Dfabric.gameJarPath*`
    /// already present in the old `launch.toml`.
    pub fabric_game_jar_property: Option<String>,
    /// When Fabric's vanilla JAR lives outside `<server>/`, verification resolves it directly
    /// against this canonical read-only host path without creating an out-of-root copy in `stage/`.
    pub verification_external_vanilla_jar: Option<PathBuf>,
}

/// Prepares the staging directory and installer CLI arguments for `family`.
pub(crate) fn prepare(
    family: LoaderFamily,
    loader_version: &str,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
    force: bool,
) -> Result<PreparedInstall> {
    match family {
        LoaderFamily::Forge | LoaderFamily::NeoForge => {
            let (installer_args, plan) = fml::prepare(family, snapshot, staging)?;
            Ok(PreparedInstall {
                installer_args,
                plan: AdapterPlan::Fml(plan),
            })
        }
        LoaderFamily::Fabric => {
            let (installer_args, plan) =
                fabric::prepare(loader_version, snapshot, staging, force)?;
            Ok(PreparedInstall {
                installer_args,
                plan: AdapterPlan::Fabric(plan),
            })
        }
    }
}

/// Collects post-install launch argv, verification inputs, and the allowlist [`PublishPlan`].
pub(crate) fn collect(
    prepared: PreparedInstall,
    snapshot: &PreflightSnapshot,
    staging: &StagingDir,
) -> Result<InstalledArtifacts> {
    match prepared.plan {
        AdapterPlan::Fml(plan) => fml::collect(plan, snapshot, staging),
        AdapterPlan::Fabric(plan) => fabric::collect(plan, snapshot, staging),
    }
}

/// Expands upstream Forge/NeoForge profile path templates (`{ROOT}`, `{LIBRARY_DIR}`,
/// `{MINECRAFT_VERSION}`) into a safe relative path strictly contained inside the install cwd.
pub(crate) fn expand_profile_path(template: &str, mc_version: &str) -> Result<PathBuf> {
    ensure!(
        !template.trim().is_empty(),
        "install_profile path template is empty"
    );
    let expanded = template
        .replace("{LIBRARY_DIR}", "libraries")
        .replace("{MINECRAFT_VERSION}", mc_version)
        .replace("{ROOT}", "");
    ensure!(
        !expanded.contains('{') && !expanded.contains('}'),
        "unsupported placeholder in install_profile path template {template:?}"
    );
    // Templates like `{ROOT}/libraries/...` leave a leading slash after replacing `{ROOT}` with "".
    let trimmed = if template.starts_with("{ROOT}") {
        expanded.trim_start_matches(['/', '\\'])
    } else {
        expanded.as_str()
    };
    validate_safe_relative_path(trimmed, "install_profile path")
}

/// Normalizes a slash/backslash-separated relative path and rejects any path traversal,
/// NUL byte, empty string, or absolute/drive-prefixed path.
pub(crate) fn validate_safe_relative_path(raw: &str, label: &str) -> Result<PathBuf> {
    ensure!(!raw.trim().is_empty(), "{label} cannot be empty");
    ensure!(!raw.contains('\0'), "{label} contains NUL: {raw:?}");
    let normalized = raw.replace('\\', "/");
    ensure!(
        !normalized.starts_with('/'),
        "{label} must be relative to the install directory: {raw:?}"
    );
    let bytes = normalized.as_bytes();
    ensure!(
        !(bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'),
        "{label} cannot contain a Windows drive prefix: {raw:?}"
    );

    let mut clean = PathBuf::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        ensure!(
            segment != "..",
            "{label} cannot escape the install directory: {raw:?}"
        );
        clean.push(segment);
    }
    ensure!(
        !clean.as_os_str().is_empty(),
        "{label} resolves to an empty path: {raw:?}"
    );
    for component in clean.components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "{label} contains an invalid path component: {raw:?}"
        );
    }
    Ok(clean)
}

/// Derives the root-directory launch JAR filename(s) from a Maven coordinate
/// (`group:artifact:version[:classifier][@ext]`).
///
/// Returns `(primary, fallback)` where `fallback` strips a `:universal` classifier because
/// Forge 1.13–1.16 installers write `{artifact}-{version}.jar` at the server root.
pub(crate) fn maven_coord_root_filenames(coord: &str) -> Result<(String, Option<String>)> {
    let (base, ext) = match coord.rsplit_once('@') {
        Some((base, ext)) => {
            ensure!(
                !ext.is_empty() && !ext.contains([':', '/', '\\']),
                "invalid Maven extension in coordinate {coord:?}"
            );
            (base, ext)
        }
        None => (coord, "jar"),
    };
    let parts: Vec<&str> = base.split(':').collect();
    ensure!(
        matches!(parts.len(), 3 | 4),
        "invalid Maven coordinate {coord:?}"
    );
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).copied();
    for segment in [group, artifact, version].into_iter().chain(classifier) {
        ensure!(
            !segment.is_empty() && !segment.contains(['/', '\\', '\0']) && segment != "..",
            "invalid Maven segment {segment:?} in coordinate {coord:?}"
        );
    }
    let plain = format!("{artifact}-{version}.{ext}");
    let (primary, fallback) = match classifier {
        None => (plain, None),
        Some("universal") => (plain.clone(), Some(format!("{artifact}-{version}-universal.{ext}"))),
        Some(classifier) => (format!("{artifact}-{version}-{classifier}.{ext}"), None),
    };
    validate_safe_relative_path(&primary, "Maven artifact filename")
        .with_context(|| format!("unsafe Maven coordinate {coord:?}"))?;
    Ok((primary, fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_profile_path_expands_known_variables_and_rejects_escapes() {
        assert_eq!(
            expand_profile_path(
                "{LIBRARY_DIR}/net/minecraft/server/{MINECRAFT_VERSION}/server-{MINECRAFT_VERSION}-bundled.jar",
                "1.21.1"
            )
            .unwrap(),
            PathBuf::from("libraries/net/minecraft/server/1.21.1/server-1.21.1-bundled.jar")
        );
        assert_eq!(
            expand_profile_path(
                "{ROOT}/libraries/net/neoforged/neoforge/21.1.72/unix_args.txt",
                "1.21.1"
            )
            .unwrap(),
            PathBuf::from("libraries/net/neoforged/neoforge/21.1.72/unix_args.txt")
        );

        for invalid in [
            "",
            "{UNKNOWN}/foo.jar",
            "/etc/passwd",
            "{ROOT}/../escape.jar",
            "libraries/../../escape.jar",
            r"C:\outside\server.jar",
        ] {
            assert!(expand_profile_path(invalid, "1.21.1").is_err(), "{invalid}");
        }
    }

    #[test]
    fn maven_coord_root_filenames_handles_plain_and_universal_coords() {
        assert_eq!(
            maven_coord_root_filenames("net.minecraftforge:forge:1.16.5-36.2.39").unwrap(),
            ("forge-1.16.5-36.2.39.jar".to_owned(), None)
        );
        assert_eq!(
            maven_coord_root_filenames("net.minecraftforge:forge:1.16.5-36.2.39:universal")
                .unwrap(),
            (
                "forge-1.16.5-36.2.39.jar".to_owned(),
                Some("forge-1.16.5-36.2.39-universal.jar".to_owned())
            )
        );
        assert!(maven_coord_root_filenames("bad:coord").is_err());
    }
}
