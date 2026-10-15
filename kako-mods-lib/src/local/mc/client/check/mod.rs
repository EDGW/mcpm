//! Read-only, offline integrity inspection of selected version resources.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use super::{
    MinecraftDirectory, MinecraftVersion, MinecraftVersionManifest, manifest::AssetIndexInfo,
};

mod file;
mod progress;
mod report;

use progress::Progress;
pub use progress::{CheckEvent, CheckProgress, CheckStep, PlannedFile};

pub use report::{
    CheckIssue, CheckProblem, CheckReport, CheckTarget, CheckedFile, FileCheckStatus,
};
pub use super::rules::{
    ClientPlatform as CheckPlatform, LibraryRuleError, RuleEnvironment as CheckRules,
};

/// Verification policy for an independently selectable resource category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOptionValue {
    Size,
    /// Check size first, then verify SHA-1. A size mismatch skips hashing.
    Hash,
    Skip,
}

impl CheckOptionValue {
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::Skip)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckOptions {
    pub libraries: CheckOptionValue,
    pub assets: CheckOptionValue,
    pub asset_index: CheckOptionValue,
    pub version_jar: CheckOptionValue,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            libraries: CheckOptionValue::Hash,
            assets: CheckOptionValue::Size,
            asset_index: CheckOptionValue::Hash,
            version_jar: CheckOptionValue::Hash,
        }
    }
}

impl CheckOptions {
    pub const fn is_empty(self) -> bool {
        !self.libraries.is_enabled()
            && !self.assets.is_enabled()
            && !self.asset_index.is_enabled()
            && !self.version_jar.is_enabled()
    }
}

pub(super) fn check(
    directory: &MinecraftDirectory,
    version: &MinecraftVersion,
    options: CheckOptions,
    rules: &CheckRules,
    mut observer: impl FnMut(CheckProgress<'_>),
) -> CheckReport {
    let mut report = CheckReport {
        version: version.name().to_owned(),
        options,
        files: Vec::new(),
        issues: Vec::new(),
    };
    let mut progress = Progress::new(&mut observer, &report);
    run(directory, version, rules, &mut report, &mut progress);
    progress.finish(&report);
    report
}

fn run(
    directory: &MinecraftDirectory,
    version: &MinecraftVersion,
    rules: &CheckRules,
    report: &mut CheckReport,
    progress: &mut Progress<'_>,
) {
    let same_root = if directory.path() == version.root_path() {
        Ok(true)
    } else {
        fs::canonicalize(directory.path())
            .and_then(|root| fs::canonicalize(version.root_path()).map(|other| root == other))
    };
    match same_root {
        Ok(true) => {}
        Ok(false) => {
            report.issues.push(CheckIssue {
                target: CheckTarget::Version,
                path: Some(version.path()),
                problem: CheckProblem::RootMismatch {
                    expected: directory.path().to_owned(),
                    actual: version.root_path().to_owned(),
                },
            });
            return;
        }
        Err(source) => {
            report.issues.push(CheckIssue {
                target: CheckTarget::Version,
                path: Some(version.path()),
                problem: CheckProblem::Io { source },
            });
            return;
        }
    }
    let manifest = match version.manifest() {
        Ok(manifest) => manifest,
        Err(source) => {
            report.issues.push(CheckIssue {
                target: CheckTarget::Version,
                path: Some(version.manifest_path()),
                problem: CheckProblem::Version { source },
            });
            return;
        }
    };
    let mut plan = Vec::new();
    if report.options.version_jar.is_enabled()
        && let Some(file) = plan_version_jar(version, manifest, report)
    {
        plan.push(file);
    }
    if report.options.libraries.is_enabled() {
        plan.extend(plan_libraries(directory, manifest, rules, report));
    }
    let index_needed =
        report.options.asset_index.is_enabled() || report.options.assets.is_enabled();
    let index = if index_needed {
        plan_asset_index(directory, manifest, report)
    } else {
        None
    };
    if report.options.asset_index.is_enabled()
        && let Some((_, file)) = &index
    {
        plan.push(file.clone());
    }
    progress.add_plan(plan, !report.options.assets.is_enabled(), report);
    for (mode, step) in [
        (report.options.version_jar, CheckStep::VersionJar),
        (report.options.libraries, CheckStep::Libraries),
    ] {
        if mode.is_enabled() {
            progress.start_step(step, report);
            while let Some(file) = progress.next_file() {
                file::check_file(report, file, progress);
            }
            progress.end_step(report);
        }
    }
    if index_needed {
        check_assets(directory, index, report, progress);
    }
}

fn plan_version_jar(
    version: &MinecraftVersion,
    manifest: &MinecraftVersionManifest,
    report: &mut CheckReport,
) -> Option<PlannedFile> {
    let path = match version.jar_path() {
        Ok(path) => path,
        Err(source) => {
            report.issues.push(CheckIssue {
                target: CheckTarget::VersionJar,
                path: None,
                problem: CheckProblem::Version { source },
            });
            return None;
        }
    };
    let download = manifest.client_download();
    Some(PlannedFile {
        step: CheckStep::VersionJar,
        target: CheckTarget::VersionJar,
        path,
        size: download.and_then(|file| file.size),
        checksums: download
            .and_then(|file| file.sha1.clone())
            .into_iter()
            .collect(),
        mode: report.options.version_jar,
    })
}

fn plan_libraries(
    directory: &MinecraftDirectory,
    manifest: &MinecraftVersionManifest,
    rules: &CheckRules,
    report: &mut CheckReport,
) -> Vec<PlannedFile> {
    let libraries = directory.libraries();
    // Select declarations before resolving file paths: superseded declarations
    // must not contribute stale metadata failures or old repository paths.
    let mut selected = BTreeMap::new();
    for library in manifest.libraries() {
        if library.clientreq == Some(false) {
            continue;
        }
        let allowed = match rules.allows(&library.rules) {
            Ok(allowed) => allowed,
            Err(source) => {
                library_issue(
                    report,
                    &library.name,
                    super::libraries::LibraryError::Rule(source),
                );
                continue;
            }
        };
        if !allowed {
            continue;
        }
        match library.coordinate() {
            Ok(coordinate) => {
                selected.insert(coordinate.key(), library);
            }
            Err(source) => library_issue(
                report,
                &library.name,
                super::libraries::LibraryError::Coordinate(source),
            ),
        }
    }
    let mut plan = Vec::new();
    for library in selected.into_values() {
        match libraries.resolve(library, rules) {
            Ok(files) => {
                for file in files {
                    let hashes = file
                        .download
                        .sha1
                        .into_iter()
                        .chain(file.checksums)
                        .collect();
                    plan.push(PlannedFile {
                        step: CheckStep::Libraries,
                        mode: report.options.libraries,
                        target: CheckTarget::Library {
                            name: file.name,
                            classifier: file.classifier,
                        },
                        path: file.path,
                        size: file.download.size,
                        checksums: hashes,
                    });
                }
            }
            Err(source) => library_issue(report, &library.name, source),
        }
    }
    plan
}

fn library_issue(report: &mut CheckReport, name: &str, source: super::libraries::LibraryError) {
    report.issues.push(CheckIssue {
        target: CheckTarget::Library {
            name: name.to_owned(),
            classifier: None,
        },
        path: None,
        problem: CheckProblem::Library { source },
    });
}

fn plan_asset_index(
    directory: &MinecraftDirectory,
    manifest: &MinecraftVersionManifest,
    report: &mut CheckReport,
) -> Option<(AssetIndexInfo, PlannedFile)> {
    let info = if let Some(info) = manifest.asset_index() {
        info.clone()
    } else if let Some(id) = manifest.assets() {
        AssetIndexInfo {
            id: id.to_owned(),
            url: None,
            sha1: None,
            size: None,
            total_size: None,
            extra: BTreeMap::new(),
        }
    } else {
        report.issues.push(CheckIssue {
            target: CheckTarget::Version,
            path: None,
            problem: CheckProblem::MissingMetadata {
                field: "assetIndex or assets",
            },
        });
        return None;
    };
    let target = CheckTarget::AssetIndex {
        id: info.id.clone(),
    };
    match directory.assets().index_path(&info.id) {
        Ok(path) => {
            let file = PlannedFile {
                step: CheckStep::AssetIndex,
                mode: report.options.asset_index,
                target,
                path,
                size: info.size,
                checksums: info.sha1.clone().into_iter().collect(),
            };
            Some((info, file))
        }
        Err(source) => {
            report.issues.push(CheckIssue {
                target,
                path: None,
                problem: CheckProblem::Assets { source },
            });
            None
        }
    }
}

fn check_assets(
    directory: &MinecraftDirectory,
    index: Option<(AssetIndexInfo, PlannedFile)>,
    report: &mut CheckReport,
    progress: &mut Progress<'_>,
) {
    progress.start_step(CheckStep::AssetIndex, report);
    let Some((info, index_file)) = index else {
        progress.end_step(report);
        return;
    };
    if report.options.asset_index.is_enabled()
        && file::check_file(report, index_file.clone(), progress) == FileCheckStatus::Failed
    {
        progress.end_step(report);
        return;
    }
    let assets = directory.assets();
    let index = match assets.read_index(&info.id) {
        Ok(index) => index,
        Err(source) => {
            report.issues.push(CheckIssue {
                target: index_file.target,
                path: Some(index_file.path),
                problem: CheckProblem::Assets { source },
            });
            progress.end_step(report);
            return;
        }
    };
    if !report.options.assets.is_enabled() {
        progress.end_step(report);
        return;
    }
    let mut checked = BTreeSet::new();
    let mut plan = Vec::new();
    for (name, object) in index.objects {
        let target = CheckTarget::AssetObject { name };
        match assets.object_path(&object.hash) {
            Ok(path) if checked.insert((path.clone(), object.size)) => {
                plan.push(PlannedFile {
                    step: CheckStep::AssetObjects,
                    mode: report.options.assets,
                    target,
                    path,
                    size: Some(object.size),
                    checksums: vec![object.hash],
                });
            }
            Ok(_) => {}
            Err(source) => report.issues.push(CheckIssue {
                target,
                path: None,
                problem: CheckProblem::Assets { source },
            }),
        }
    }
    progress.add_plan(plan, true, report);
    progress.start_step(CheckStep::AssetObjects, report);
    while let Some(file) = progress.next_file() {
        file::check_file(report, file, progress);
    }
    progress.end_step(report);
}
