use std::{fs, path::Path};

use kako_mods_lib::local::mc::client::{
    AssetError, CheckEvent, CheckOptionValue, CheckOptions, CheckPlatform, CheckProblem,
    CheckRules, CheckStep, CheckTarget, FileCheckStatus, LibraryError, MinecraftDirectory,
    MinecraftVersionManifest,
};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use tempfile::TempDir;

const SKIP_ALL: CheckOptions = CheckOptions {
    libraries: CheckOptionValue::Skip,
    assets: CheckOptionValue::Skip,
    asset_index: CheckOptionValue::Skip,
    version_jar: CheckOptionValue::Skip,
};
const LIBRARIES: CheckOptions = CheckOptions {
    libraries: CheckOptionValue::Hash,
    ..SKIP_ALL
};
const ASSETS: CheckOptions = CheckOptions {
    assets: CheckOptionValue::Hash,
    asset_index: CheckOptionValue::Hash,
    ..SKIP_ALL
};
const RESOURCES: CheckOptions = CheckOptions {
    libraries: CheckOptionValue::Hash,
    ..ASSETS
};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn profile(root: &Path, name: &str, value: Value) {
    write(
        root,
        &format!("versions/{name}/{name}.json"),
        value.to_string().as_bytes(),
    );
}
fn rules() -> CheckRules {
    CheckRules::new(CheckPlatform {
        os: "linux".into(),
        arch: "amd64".into(),
        version: "6.8".into(),
        bits: 64,
    })
}
fn artifact(path: &str, bytes: &[u8]) -> Value {
    json!({"path":path, "url":format!("https://example.invalid/{path}"), "size":bytes.len(), "sha1":sha(bytes)})
}
fn index(root: &Path, objects: Value) -> Value {
    let bytes = json!({"objects":objects, "virtual":true, "map_to_resources":true})
        .to_string()
        .into_bytes();
    write(root, "assets/indexes/test.json", &bytes);
    json!({"id":"test", "size":bytes.len(), "sha1":sha(&bytes), "url":"https://example.invalid/index", "totalSize":3})
}

#[test]
fn complete_metadata_inherits_and_retains_downloads_rules_and_asset_extensions() {
    let root = TempDir::new().unwrap();
    profile(
        root.path(),
        "parent",
        json!({"id":"parent", "assets":"test", "assetIndex":{"id":"test","sha1":sha(b"index"),"size":5,"totalSize":42,"url":"https://example.invalid/index","custom":1}, "libraries":[{
            "name":"org.example:lib:1:api@zip", "url":"https://example.invalid/maven", "checksums":[sha(b"lib")],
            "downloads":{"artifact":artifact("main.zip",b"lib"),"classifiers":{"natives-linux-64":artifact("native.zip",b"native")}},
            "natives":{"linux":"natives-linux-${arch}"}, "rules":[{"action":"allow","os":{"name":"linux","arch":"amd64","version":"6\\..*"},"features":{"demo":true}}],
            "extract":{"exclude":["META-INF/"]},"clientreq":true,"serverreq":false,"extension":{"enabled":true}
        }]}),
    );
    profile(
        root.path(),
        "child",
        json!({"id":"child","inheritsFrom":"parent"}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("child").unwrap();
    let manifest = version.manifest().unwrap();
    let library = manifest.libraries().next().unwrap();
    assert_eq!(library.downloads.as_ref().unwrap().classifiers.len(), 1);
    assert_eq!(library.extract.as_ref().unwrap().exclude, ["META-INF/"]);
    assert_eq!(library.rules[0].features.get("demo"), Some(&true));
    assert_eq!(library.extra["extension"]["enabled"], true);
    assert_eq!(
        library.coordinate().unwrap().path().unwrap(),
        Path::new("org/example/lib/1/lib-1-api.zip")
    );
    assert_eq!(manifest.asset_index().unwrap().total_size, Some(42));
    assert_eq!(manifest.asset_index().unwrap().extra["custom"], 1);
    assert_eq!(manifest.assets(), Some("test"));
    profile(
        root.path(),
        "child",
        json!({"id":"child","inheritsFrom":"parent","assets":"other"}),
    );
    assert!(
        directory
            .version("child")
            .unwrap()
            .manifest()
            .unwrap()
            .asset_index()
            .is_none()
    );
}

#[test]
fn checks_only_selected_platform_libraries_natives_and_latest_patch_versions() {
    let root = TempDir::new().unwrap();
    write(root.path(), "libraries/current.jar", b"lib");
    write(root.path(), "libraries/linux.jar", b"native");
    profile(
        root.path(),
        "test",
        json!({"id":"test", "libraries":[
        {"name":"g:lib:old", "downloads":{"artifact":artifact("../superseded-invalid.jar",b"old")}},
        {"name":"g:windows:1", "downloads":{"artifact":artifact("absent-windows.jar",b"win")},"rules":[{"action":"allow","os":{"name":"windows"}}]},
        {"name":"g:native:1", "downloads":{"classifiers":{"natives-linux-64":artifact("linux.jar",b"native"),"natives-windows-64":artifact("absent-win-native.jar",b"win")}},"natives":{"linux":"natives-linux-${arch}","windows":"natives-windows-${arch}"}}
    ], "patches":[{"id":"update","libraries":[{"name":"g:lib:new","downloads":{"artifact":artifact("current.jar",b"lib")}}]}]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let report = directory.check(&directory.version("test").unwrap(), LIBRARIES, &rules());
    assert!(report.is_intact(), "{report:#?}");
    assert_eq!(report.options, LIBRARIES);
    assert_eq!(report.files.len(), 2);
    assert_eq!(directory.libraries().path(), root.path().join("libraries"));
}

#[test]
fn rules_use_caller_platform_features_and_last_matching_action() {
    let manifest: MinecraftVersionManifest =
        serde_json::from_value(json!({"id":"test","libraries":[{
            "name":"g:lib:1", "rules":[
                {"action":"allow"},
                {"action":"disallow","os":{"name":"linux","arch":"amd64","version":"6\\..*"}},
                {"action":"allow","features":{"demo":true}}
            ]
        }]}))
        .unwrap();
    let library = manifest.libraries().next().unwrap();
    let mut environment = rules();
    assert!(!environment.allows(&library.rules).unwrap());
    environment.features.insert("demo".into(), true);
    assert!(environment.allows(&library.rules).unwrap());
    environment.features.clear();
    environment.platform.os = "osx".into();
    assert!(environment.allows(&library.rules).unwrap());
    environment.platform.os = "linux".into();
    environment.platform.arch = "arm64".into();
    assert!(environment.allows(&library.rules).unwrap());
}

#[test]
fn report_collects_missing_size_hash_and_file_type_errors_without_false_integrity() {
    let root = TempDir::new().unwrap();
    write(root.path(), "libraries/corrupt.jar", b"bad!");
    write(root.path(), "libraries/no-hash.jar", b"exists");
    write(root.path(), "libraries/corrupt-same-size.jar", b"bad!");
    fs::create_dir_all(root.path().join("libraries/not-file.jar")).unwrap();
    profile(
        root.path(),
        "test",
        json!({"id":"test","libraries":[
            {"name":"g:missing:1","downloads":{"artifact":artifact("missing.jar",b"good")}},
            {"name":"g:corrupt:1","downloads":{"artifact":artifact("corrupt.jar",b"expected")}},
        {"name":"g:hash:1","downloads":{"artifact":artifact("corrupt-same-size.jar",b"good")}},
            {"name":"g:directory:1","downloads":{"artifact":artifact("not-file.jar",b"good")}},
            {"name":"g:unknown:1","downloads":{"artifact":{"path":"no-hash.jar","size":6}}}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let report = directory.check(&directory.version("test").unwrap(), LIBRARIES, &rules());
    assert!(!report.is_intact());
    assert!(report.has_errors());
    assert_eq!(report.unverified_count(), 1);
    assert_eq!(report.files.len(), 5);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue.problem, CheckProblem::MissingFile))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue.problem, CheckProblem::SizeMismatch { .. }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue.problem, CheckProblem::HashMismatch { .. }))
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue.problem, CheckProblem::NotAFile))
    );
    assert!(
        report
            .issues
            .iter()
            .all(|issue| matches!(issue.target, CheckTarget::Library { .. }))
    );
}

#[test]
fn asset_index_and_unique_objects_are_verified_and_corruption_is_reported() {
    let root = TempDir::new().unwrap();
    let hash = sha(b"abc");
    let info = index(
        root.path(),
        json!({"one":{"hash":hash,"size":3},"two":{"hash":hash,"size":3}}),
    );
    write(
        root.path(),
        &format!("assets/objects/{}/{hash}", &hash[..2]),
        b"abc",
    );
    profile(root.path(), "test", json!({"id":"test","assetIndex":info}));
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("test").unwrap();
    let report = directory.check(&version, ASSETS, &rules());
    assert!(report.is_intact(), "{report:#?}");
    assert_eq!(report.files.len(), 2);
    let parsed = directory.assets().read_index("test").unwrap();
    assert!(parsed.virtual_assets && parsed.map_to_resources);
    write(
        root.path(),
        &format!("assets/objects/{}/{hash}", &hash[..2]),
        b"bad",
    );
    let report = directory.check(&version, ASSETS, &rules());
    assert!(report.issues.iter().any(|issue| matches!(
        issue.target,
        CheckTarget::AssetObject { .. }
    ) && matches!(
        issue.problem,
        CheckProblem::HashMismatch { .. }
    )));
    write(root.path(), "assets/indexes/test.json", b"{}");
    let report = directory.check(&version, ASSETS, &rules());
    assert!(!report.is_intact());
    assert_eq!(
        report.files.len(),
        1,
        "untrusted corrupt index must not drive object checks"
    );
}

#[test]
fn legacy_maven_downloads_checksums_and_unverified_assets_are_supported() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "libraries/org/example/demo/1/demo-1-api.zip",
        b"abc",
    );
    write(
        root.path(),
        "libraries/org/example/native/1/native-1-natives-linux-64.jar",
        b"native",
    );
    index(root.path(), json!({}));
    profile(
        root.path(),
        "test",
        json!({"id":"test","assets":"test","libraries":[
            {"name":"org.example:demo:1:api@zip","url":"https://example.invalid/","checksums":[sha(b"old"),sha(b"abc")]},
            {"name":"org.example:native:1","natives":{"linux":"natives-linux-${arch}"}}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("test").unwrap();
    let libraries = directory.check(&version, LIBRARIES, &rules());
    assert!(!libraries.has_errors(), "{libraries:#?}");
    assert_eq!(libraries.unverified_count(), 1);
    assert!(!libraries.is_intact());
    let assets = directory.check(&version, ASSETS, &rules());
    assert!(!assets.has_errors());
    assert_eq!(assets.files[0].status, FileCheckStatus::Unverified);
}

#[test]
fn invalid_metadata_is_reported_and_never_used_as_an_outside_cache_path() {
    let root = TempDir::new().unwrap();
    profile(
        root.path(),
        "test",
        json!({"id":"test","assetIndex":{"id":"../outside"},"libraries":[
            {"name":"g:escape:1","downloads":{"artifact":{"path":"../outside.jar"}}},
            {"name":"g:incomplete:1","downloads":{}},
            {"name":"g:rules:1","rules":[{"action":"allow","os":{"arch":"["}}]},
            {"name":"g:native:1","downloads":{"classifiers":{}},"natives":{"linux":"missing-classifier"}}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let report = directory.check(&directory.version("test").unwrap(), RESOURCES, &rules());
    assert_eq!(report.issues.len(), 5, "{report:#?}");
    assert!(report.files.is_empty());
    assert!(report.issues.iter().any(|issue| matches!(
        issue.problem,
        CheckProblem::Library {
            source: LibraryError::Path(_)
        }
    )));
    assert!(report.issues.iter().any(|issue| matches!(
        issue.problem,
        CheckProblem::Library {
            source: LibraryError::Rule(_)
        }
    )));
    assert!(directory.assets().object_path("../../outside").is_err());
    assert!(directory.libraries().file_path("/outside.jar").is_err());
}

#[test]
fn broken_profiles_wrong_roots_and_malformed_or_missing_indexes_are_not_intact() {
    let root = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let broken = directory.version("missing").unwrap();
    let report = directory.check(&broken, LIBRARIES, &rules());
    assert!(matches!(
        report.issues[0].problem,
        CheckProblem::Version { .. }
    ));
    profile(
        root.path(),
        "test",
        json!({"id":"test","assetIndex":{"id":"test"}}),
    );
    let version = directory.version("test").unwrap();
    let wrong = MinecraftDirectory::new(other.path())
        .unwrap()
        .check(&version, LIBRARIES, &rules());
    assert!(matches!(
        wrong.issues[0].problem,
        CheckProblem::RootMismatch { .. }
    ));
    assert!(!directory.check(&version, SKIP_ALL, &rules()).is_intact());
    let missing = directory.check(&version, ASSETS, &rules());
    assert!(matches!(
        missing.issues[0].problem,
        CheckProblem::MissingFile
    ));
    write(root.path(), "assets/indexes/test.json", b"{");
    let malformed = directory.check(&version, ASSETS, &rules());
    assert!(malformed.issues.iter().any(|issue| matches!(
        issue.problem,
        CheckProblem::Assets {
            source: AssetError::ParseIndex { .. }
        }
    )));
}

#[test]
fn child_platform_override_keeps_parent_libraries_for_other_platforms() {
    let root = TempDir::new().unwrap();
    write(root.path(), "libraries/linux.jar", b"linux");
    write(root.path(), "libraries/windows.jar", b"windows");
    profile(
        root.path(),
        "parent",
        json!({"id":"parent","libraries":[
            {"name":"g:lib:1","downloads":{"artifact":artifact("linux.jar",b"linux")}}
        ]}),
    );
    profile(
        root.path(),
        "child",
        json!({"id":"child","inheritsFrom":"parent","libraries":[
            {"name":"g:lib:2","downloads":{"artifact":artifact("windows.jar",b"windows")},"rules":[{"action":"allow","os":{"name":"windows"}}]}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("child").unwrap();
    let linux = directory.check(&version, LIBRARIES, &rules());
    assert!(linux.is_intact(), "{linux:#?}");
    assert_eq!(linux.files.len(), 1);
    assert!(linux.files[0].path.ends_with("linux.jar"));
    let mut windows_rules = rules();
    windows_rules.platform.os = "windows".into();
    let windows = directory.check(&version, LIBRARIES, &windows_rules);
    assert!(windows.is_intact(), "{windows:#?}");
    assert_eq!(windows.files.len(), 1);
    assert!(windows.files[0].path.ends_with("windows.jar"));
}

#[test]
fn progress_reports_completed_and_pending_files_only_after_each_file_finishes() {
    let root = TempDir::new().unwrap();
    let large = vec![b'x'; 200_000];
    write(root.path(), "libraries/large.jar", &large);
    write(root.path(), "libraries/unverified.jar", b"abc");
    let hash = sha(b"abc");
    let info = index(
        root.path(),
        json!({"sound":{"hash":hash,"size":3},"duplicate":{"hash":hash,"size":3}}),
    );
    write(
        root.path(),
        &format!("assets/objects/{}/{hash}", &hash[..2]),
        b"abc",
    );
    profile(
        root.path(),
        "progress",
        json!({"id":"progress","assetIndex":info,"libraries":[
            {"name":"g:large:1","downloads":{"artifact":artifact("large.jar",&large)}},
            {"name":"g:missing:1","downloads":{"artifact":artifact("missing.jar",b"abc")}},
            {"name":"g:unverified:1","downloads":{"artifact":{"path":"unverified.jar"}}}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("progress").unwrap();
    let mut events = Vec::new();
    let mut finished_files = Vec::new();
    let mut steps = Vec::new();
    let mut discovered_objects = false;
    let report = directory.check_with_progress(&version, RESOURCES, &rules(), |progress| {
        assert_eq!(progress.completed.len(), progress.completed_files);
        assert!(progress.step_number <= progress.total_steps);
        if let Some(total) = progress.total_files {
            assert_eq!(total, progress.completed_files + progress.pending.len());
            assert_eq!(progress.remaining_files, Some(progress.pending.len()));
        } else {
            assert!(progress.remaining_files.is_none());
        }
        for file in progress.completed {
            assert!(
                !progress
                    .pending
                    .iter()
                    .any(|pending| pending.path == file.path)
            );
        }
        if let Some(next) = progress.next_file {
            assert_eq!(next.path, progress.pending[0].path);
            assert_eq!(next.step, progress.step);
        }
        if progress.event == CheckEvent::StepStarted {
            steps.push(progress.step);
        }
        if progress.event == CheckEvent::PlanUpdated && progress.step == CheckStep::AssetIndex {
            assert_eq!(progress.total_files, Some(5));
            discovered_objects = true;
        }
        if let CheckEvent::FileFinished { status } = progress.event {
            assert_eq!(progress.completed_files, finished_files.len() + 1);
            let file = progress.completed.last().unwrap();
            assert_eq!(status, file.status);
            finished_files.push(file.path.clone());
        }
        if progress.event == CheckEvent::Finished {
            assert_eq!(progress.completed_steps, 4);
            assert_eq!(progress.remaining_files, Some(0));
            assert!(progress.pending.is_empty());
        }
        events.push(progress.event);
    });
    assert_eq!(
        steps,
        [
            CheckStep::Preparing,
            CheckStep::Libraries,
            CheckStep::AssetIndex,
            CheckStep::AssetObjects
        ]
    );
    assert_eq!(events.first(), Some(&CheckEvent::Started));
    assert_eq!(events.last(), Some(&CheckEvent::Finished));
    assert!(discovered_objects);
    assert_eq!(
        finished_files.len(),
        5,
        "even large files emit exactly one file event"
    );
    assert_eq!(report.files.len(), 5);
    assert!(report.has_errors());
    assert_eq!(report.unverified_count(), 1);
}

#[test]
fn progress_finishes_failed_index_and_broken_version_without_claiming_unknown_files_checked() {
    let root = TempDir::new().unwrap();
    profile(
        root.path(),
        "missing-index",
        json!({"id":"missing-index","assetIndex":{"id":"absent"}}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    for (name, expected_files) in [("missing-index", 1), ("broken", 0)] {
        let version = directory.version(name).unwrap();
        let mut finished = false;
        let mut files = 0;
        let report = directory.check_with_progress(&version, ASSETS, &rules(), |progress| {
            if let CheckEvent::FileFinished { status } = progress.event {
                assert_eq!(status, FileCheckStatus::Failed);
                files += 1;
            }
            if progress.event == CheckEvent::Finished {
                assert_eq!(progress.completed_files, expected_files);
                assert!(progress.total_files.is_none());
                assert!(progress.remaining_files.is_none());
                assert!(progress.completed_steps < progress.total_steps);
                finished = true;
            }
        });
        assert!(finished);
        assert_eq!(files, expected_files);
        assert!(!report.is_intact());
    }
}

#[test]
fn default_policies_check_jar_libraries_index_hashes_and_only_asset_sizes() {
    let root = TempDir::new().unwrap();
    let hash = sha(b"abc");
    let info = index(root.path(), json!({"sound":{"hash":hash,"size":3}}));
    write(
        root.path(),
        &format!("assets/objects/{}/{hash}", &hash[..2]),
        b"bad",
    );
    write(root.path(), "libraries/lib.jar", b"lib");
    write(root.path(), "versions/test/test.jar", b"jar");
    profile(
        root.path(),
        "test",
        json!({"id":"test","assetIndex":info,
        "downloads":{"client":{"sha1":sha(b"jar"),"size":3}},
        "libraries":[{"name":"g:lib:1","downloads":{"artifact":artifact("lib.jar",b"lib")}}]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("test").unwrap();
    let mut steps = Vec::new();
    let report =
        directory.check_with_progress(&version, CheckOptions::default(), &rules(), |progress| {
            assert_eq!(progress.total_steps, 5);
            if progress.event == CheckEvent::StepStarted {
                steps.push(progress.step);
            }
        });
    assert!(report.is_intact(), "{report:#?}");
    assert_eq!(report.files.len(), 4);
    assert_eq!(
        steps,
        [
            CheckStep::Preparing,
            CheckStep::VersionJar,
            CheckStep::Libraries,
            CheckStep::AssetIndex,
            CheckStep::AssetObjects
        ]
    );
    assert!(report.files.iter().any(
        |file| matches!(file.target, CheckTarget::AssetObject { .. })
            && file.mode == CheckOptionValue::Size
    ));
    let hash_report = directory.check(
        &version,
        CheckOptions {
            assets: CheckOptionValue::Hash,
            ..CheckOptions::default()
        },
        &rules(),
    );
    assert!(hash_report.issues.iter().any(|issue| matches!(
        issue.target,
        CheckTarget::AssetObject { .. }
    ) && matches!(
        issue.problem,
        CheckProblem::HashMismatch { .. }
    )));
}

#[test]
fn size_hash_and_skip_policies_are_distinct_and_size_failure_prevents_hash_verification() {
    let root = TempDir::new().unwrap();
    write(root.path(), "libraries/lib.jar", b"bad");
    profile(
        root.path(),
        "test",
        json!({"id":"test","libraries":[{"name":"g:lib:1","downloads":{"artifact":artifact("lib.jar",b"abc")}}]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("test").unwrap();
    let size = directory.check(
        &version,
        CheckOptions {
            libraries: CheckOptionValue::Size,
            ..SKIP_ALL
        },
        &rules(),
    );
    assert!(size.is_intact(), "{size:#?}");
    let hash = directory.check(&version, LIBRARIES, &rules());
    assert!(matches!(
        hash.issues[0].problem,
        CheckProblem::HashMismatch { .. }
    ));
    write(root.path(), "libraries/lib.jar", b"different size");
    let short_circuit = directory.check(&version, LIBRARIES, &rules());
    assert_eq!(short_circuit.issues.len(), 1);
    assert!(matches!(
        short_circuit.issues[0].problem,
        CheckProblem::SizeMismatch { .. }
    ));
    let skipped = directory.check(&version, SKIP_ALL, &rules());
    assert!(skipped.files.is_empty() && skipped.issues.is_empty());
    profile(
        root.path(),
        "no-size",
        json!({"id":"no-size","libraries":[{"name":"g:lib:1","downloads":{"artifact":{"path":"lib.jar","sha1":sha(b"different size")}}}]}),
    );
    let report = directory.check(
        &directory.version("no-size").unwrap(),
        CheckOptions {
            libraries: CheckOptionValue::Size,
            ..SKIP_ALL
        },
        &rules(),
    );
    assert_eq!(report.unverified_count(), 1);
    assert!(!report.is_intact());
}

#[test]
fn skipped_index_is_read_only_as_an_asset_dependency_and_index_can_be_checked_alone() {
    let root = TempDir::new().unwrap();
    let hash = sha(b"abc");
    let mut info = index(root.path(), json!({"sound":{"hash":hash,"size":3}}));
    info["sha1"] = json!("0".repeat(40));
    write(
        root.path(),
        &format!("assets/objects/{}/{hash}", &hash[..2]),
        b"abc",
    );
    profile(root.path(), "test", json!({"id":"test","assetIndex":info}));
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("test").unwrap();
    let assets = directory.check(
        &version,
        CheckOptions {
            assets: CheckOptionValue::Hash,
            ..SKIP_ALL
        },
        &rules(),
    );
    assert!(assets.is_intact(), "{assets:#?}");
    assert_eq!(assets.files.len(), 1);
    assert!(matches!(
        assets.files[0].target,
        CheckTarget::AssetObject { .. }
    ));
    let index_only = directory.check(
        &version,
        CheckOptions {
            asset_index: CheckOptionValue::Hash,
            ..SKIP_ALL
        },
        &rules(),
    );
    assert_eq!(index_only.files.len(), 1);
    assert!(matches!(
        index_only.issues[0].problem,
        CheckProblem::HashMismatch { .. }
    ));
}

#[test]
fn client_jar_check_resolves_inherited_and_explicit_jar_locations() {
    let root = TempDir::new().unwrap();
    profile(
        root.path(),
        "parent",
        json!({"id":"different-json-id","downloads":{"client":{"sha1":sha(b"jar"),"size":3}}}),
    );
    profile(
        root.path(),
        "child",
        json!({"id":"child","inheritsFrom":"parent"}),
    );
    write(root.path(), "versions/parent/parent.jar", b"jar");
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let options = CheckOptions {
        version_jar: CheckOptionValue::Hash,
        ..SKIP_ALL
    };
    let child = directory.version("child").unwrap();
    assert_eq!(
        child.jar_path().unwrap(),
        root.path().join("versions/parent/parent.jar")
    );
    assert!(directory.check(&child, options, &rules()).is_intact());
    profile(
        root.path(),
        "child",
        json!({"id":"child","inheritsFrom":"parent","jar":"copy"}),
    );
    write(root.path(), "versions/copy/copy.jar", b"jar");
    let aliased = directory.version("child").unwrap();
    assert_eq!(
        aliased.jar_path().unwrap(),
        root.path().join("versions/copy/copy.jar")
    );
    assert!(directory.check(&aliased, options, &rules()).is_intact());
    write(root.path(), "versions/copy/copy.jar", b"bad");
    let report = directory.check(&aliased, options, &rules());
    assert!(matches!(report.issues[0].target, CheckTarget::VersionJar));
    assert!(matches!(
        report.issues[0].problem,
        CheckProblem::HashMismatch { .. }
    ));
    profile(
        root.path(),
        "unsafe",
        json!({"id":"unsafe","jar":"../outside"}),
    );
    assert!(
        directory
            .check(&directory.version("unsafe").unwrap(), options, &rules())
            .has_errors()
    );
}
