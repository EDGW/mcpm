//! Cache CLI workflows, batch failure recovery and content/metadata ownership.

use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::Value;

fn run(root: &Path, cache: &Path, arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args([
            "--raw",
            "--log-level",
            "off",
            "cache",
            "mods",
            "--directory",
        ])
        .arg(cache)
        .args(arguments)
        .current_dir(root)
        .env("HOME", root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn imports(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let cells: Vec<_> = line.split('\t').collect();
            serde_json::json!({"id":cells[0], "data_path":cells[1], "published":cells[2] == "true",
                "method": if cells[3].is_empty() { Value::Null } else { Value::String(cells[3].into()) }})
        })
        .collect()
}

fn batch(root: &Path, cache: &Path, arguments: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args([
            "--raw",
            "--log-level",
            "off",
            "cache",
            "mods",
            "--directory",
        ])
        .arg(cache)
        .args(arguments)
        .current_dir(root)
        .env("HOME", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn batch_refresh_and_queries_keep_content_and_named_metadata_independent() {
    use mcpm_backend::{
        local::cache::{
            CacheLinkPreference,
            mods::{CacheStatus, CacheUnavailableReason, ModsCacheDir},
        },
        source::{
            FileHash, FileId, FileMatch, FileRef, FileSummary, MatchedFileRole, ProjectId,
            ProjectRef, SourceId,
        },
    };
    let root = tempfile::tempdir().unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let other_fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dependency-problems/mods/fabric_only.jar");
    let first = cache.import(&fixture, CacheLinkPreference::Copy).unwrap();
    let second = cache
        .import(&other_fixture, CacheLinkPreference::Copy)
        .unwrap();
    assert!(cache.inspect(&first).unwrap().metadata.is_empty());
    let summary = FileSummary::read(&fixture, Default::default()).unwrap();
    for source in ["modrinth", "curseforge"] {
        cache
            .write_ident_info(
                first,
                &FileMatch {
                    reference: FileRef {
                        project: ProjectRef {
                            source: SourceId(source.into()),
                            id: ProjectId("project".into()),
                        },
                        id: FileId("release".into()),
                    },
                    filename: "fixture.jar".into(),
                    size: summary.size,
                    hashes: vec![FileHash::Sha1(summary.sha1.clone())],
                    role: MatchedFileRole::Primary,
                },
            )
            .unwrap();
    }
    let ident = cache.ident_info(first);
    let original_ident = fs::read(ident.path()).unwrap();
    let first_text = first.to_string();
    let second_text = second.to_string();
    let refreshed = batch(
        root.path(),
        cache.path(),
        &["refresh", &first_text, "meta", "--loader", "fabric"],
        &format!("{second}\n\n{first}\n"),
    );
    assert!(refreshed.status.success(), "{refreshed:?}");
    assert_eq!(cache.mod_metadata(second).status(), &CacheStatus::Available);
    assert_eq!(
        cache
            .mod_metadata(first)
            .digest()
            .unwrap()
            .identifications()
            .len(),
        1
    );
    assert_eq!(fs::read(ident.path()).unwrap(), original_ident);
    let meta = cache.mod_metadata(first);
    let original_meta = fs::read(meta.digest_path()).unwrap();
    let invalid = batch(
        root.path(),
        cache.path(),
        &["refresh", &first_text, "meta", "--force"],
        "invalid-hash\n",
    );
    assert!(!invalid.status.success());
    assert_eq!(fs::read(meta.digest_path()).unwrap(), original_meta);
    fs::write(meta.digest_path(), b"broken metadata").unwrap();
    let identified = batch(
        root.path(),
        cache.path(),
        &[
            "refresh",
            &first_text,
            "ident",
            "--source",
            "curseforge",
            "--source",
            "modrinth",
        ],
        "",
    );
    assert!(identified.status.success(), "{identified:?}");
    assert_eq!(fs::read(ident.path()).unwrap(), original_ident);
    let info = batch(
        root.path(),
        cache.path(),
        &["info", &first_text, "--columns", "status,metadatas"],
        "",
    );
    assert!(info.status.success(), "{info:?}");
    assert_eq!(
        String::from_utf8(info.stdout).unwrap(),
        "available\tident,meta\n"
    );
    assert_eq!(
        cache.mod_metadata(first).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    assert_eq!(cache.ident_info(first).status(), &CacheStatus::Available);
    fs::write(ident.path(), b"broken metadata").unwrap();
    let verified = batch(
        root.path(),
        cache.path(),
        &["verify", &first_text],
        &format!("{second}\n"),
    );
    assert!(verified.status.success(), "{verified:?}");
    let listed = batch(root.path(), cache.path(), &["list"], "");
    assert!(listed.status.success());
    let listed = String::from_utf8(listed.stdout).unwrap();
    let expected = {
        let mut ids = [first_text, second_text];
        ids.sort();
        format!("{}\n{}\n", ids[0], ids[1])
    };
    assert_eq!(listed, expected);
    assert_eq!(fs::read(meta.digest_path()).unwrap(), b"broken metadata");
    assert_eq!(fs::read(ident.path()).unwrap(), b"broken metadata");
}

#[test]
fn all_selection_rejects_mixed_inputs_and_includes_orphan_and_invalid_entries() {
    use mcpm_backend::local::cache::{
        CacheDir, CacheLinkPreference,
        codec::CacheDirMetadata,
        hash::Blake3Hash,
        mods::{CacheStatus, ModsCacheDir},
    };

    let root = tempfile::tempdir().unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    for arguments in [
        vec!["info", "--all"],
        vec!["verify", "--all"],
        vec!["refresh", "--all", "meta"],
        vec!["refresh", "ident", "--all"],
    ] {
        let empty = batch(root.path(), cache.path(), &arguments, "\n");
        assert!(empty.status.success(), "{empty:?}");
    }
    assert!(cache.scan().unwrap().is_empty());

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let ids = [
        "complex-mod.jar",
        "dependency-problems/mods/fabric_only.jar",
    ]
    .map(|name| {
        cache
            .import(&fixtures.join(name), CacheLinkPreference::Copy)
            .unwrap()
    });
    let first_text = ids[0].to_string();
    for arguments in [
        vec!["info", "--all"],
        vec!["verify", "--all"],
        vec!["refresh", "--all", "meta"],
        vec!["refresh", "ident", "--all"],
    ] {
        // Reject all nonblank stdin before refreshing any entry, even malformed IDs.
        for input in [first_text.as_str(), "invalid-hash"] {
            let rejected = batch(root.path(), cache.path(), &arguments, input);
            assert!(!rejected.status.success(), "{rejected:?}");
        }
    }
    for arguments in [
        vec!["info", &first_text, "--all"],
        vec!["verify", &first_text, "--all"],
        vec!["refresh", &first_text, "meta", "--all"],
        vec!["refresh", "--all", &first_text, "ident"],
    ] {
        let rejected = batch(root.path(), cache.path(), &arguments, "");
        assert!(!rejected.status.success(), "{rejected:?}");
    }
    for id in ids {
        assert!(cache.inspect(&id).unwrap().metadata.is_empty());
    }

    let refreshed = batch(
        root.path(),
        cache.path(),
        &["refresh", "meta", "--all", "--loader", "fabric"],
        "",
    );
    assert!(refreshed.status.success(), "{refreshed:?}");
    for id in ids {
        let metadata = cache.mod_metadata(id);
        assert_eq!(metadata.status(), &CacheStatus::Available);
        let mut checked = metadata.digest().unwrap().identifications();
        assert_eq!(checked.len(), 1);
        assert_eq!(checked.next().unwrap().0, "fabric");
    }
    let verified = batch(root.path(), cache.path(), &["verify", "--all"], "");
    assert!(verified.status.success(), "{verified:?}");

    // Deliberately orphan metadata to ensure --all uses diagnostic inventory, not valid contents only.
    let generic = CacheDir::new(cache.path()).unwrap();
    generic
        .write_metadata(&ids[0], "custom", &CacheDirMetadata::new())
        .unwrap()
        .unwrap();
    fs::remove_file(cache.content_path(&ids[0])).unwrap();
    let info = batch(
        root.path(),
        cache.path(),
        &["info", "--all", "--columns", "blake3,status,metadatas"],
        "",
    );
    assert!(info.status.success(), "{info:?}");
    let mut expected = [
        format!("{}\tnot_found\tcustom,meta\n", ids[0]),
        format!("{}\tavailable\tmeta\n", ids[1]),
    ];
    expected.sort();
    assert_eq!(String::from_utf8(info.stdout).unwrap(), expected.concat());
    let orphaned = batch(root.path(), cache.path(), &["verify", "--all"], "");
    assert!(!orphaned.status.success(), "{orphaned:?}");

    let invalid = Blake3Hash::new([0; 32]);
    fs::create_dir_all(cache.content_path(&invalid)).unwrap();
    let inventory = batch(
        root.path(),
        cache.path(),
        &["info", "--all", "--columns", "blake3,status"],
        "",
    );
    assert!(inventory.status.success(), "{inventory:?}");
    assert!(
        String::from_utf8(inventory.stdout)
            .unwrap()
            .starts_with(&format!("{invalid}\tunavailable\n"))
    );
    let invalid_content = batch(root.path(), cache.path(), &["verify", "--all"], "");
    assert!(!invalid_content.status.success(), "{invalid_content:?}");
}

#[test]
fn verification_and_autoremove_continue_after_content_and_removal_failures() {
    use mcpm_backend::local::cache::{
        CacheDir, CacheLinkPreference, codec::CacheDirMetadata, hash::Blake3Hash,
        mods::ModsCacheDir,
    };

    // Exercise explicit sequential inputs as well as sequential and concurrent inventories.
    for parallelism in [None, Some("1"), Some("2")] {
        let root = tempfile::tempdir().unwrap();
        let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
        let ids = [b"valid content", b"other content", b"orphan source"].map(|bytes| {
            let source = root.path().join("source.jar");
            fs::write(&source, bytes).unwrap();
            cache.import(&source, CacheLinkPreference::Copy).unwrap()
        });
        let [valid, corrupt, orphan] = ids;
        let generic = CacheDir::new(cache.path()).unwrap();
        for id in ids {
            generic
                .write_metadata(&id, "custom", &CacheDirMetadata::new())
                .unwrap()
                .unwrap();
        }
        // Replace the immutable cached file to simulate corruption without changing its permissions.
        fs::remove_file(cache.content_path(&corrupt)).unwrap();
        fs::write(cache.content_path(&corrupt), b"broken content").unwrap();
        let external = root.path().join("external.jar");
        fs::hard_link(cache.content_path(&corrupt), &external).unwrap();
        fs::remove_file(cache.content_path(&orphan)).unwrap();
        let invalid = Blake3Hash::new([0; 32]);
        fs::create_dir_all(cache.content_path(&invalid)).unwrap();
        let invalid_child = cache.content_path(&invalid).join("retained");
        fs::write(&invalid_child, b"do not recursively remove").unwrap();
        let absent = Blake3Hash::new([255; 32]);
        let explicit = [invalid, corrupt, valid, orphan, absent].map(|id| id.to_string());
        let mut arguments = vec!["verify"];
        if let Some(parallelism) = parallelism {
            arguments.extend(["--all", "--parallelism", parallelism]);
        } else {
            arguments.extend(explicit.iter().map(String::as_str));
        }
        let verified = batch(root.path(), cache.path(), &arguments, "");
        assert!(!verified.status.success(), "{verified:?}");
        let output = String::from_utf8(verified.stdout).unwrap();
        // Both later failures must be checked after the invalid first entry.
        assert!(output.contains(&format!("{corrupt}\tunavailable\t\n")));
        assert!(output.contains(&format!("{orphan}\tnot_found\t\n")));
        assert!(!output.contains(&valid.to_string()));
        assert!(cache.content_path(&corrupt).is_file());
        assert!(
            cache
                .inspect(&orphan)
                .unwrap()
                .metadata
                .iter()
                .any(|metadata| metadata.name == "custom")
        );

        arguments.extend(["--autoremove", "--details"]);
        let removed = batch(root.path(), cache.path(), &arguments, "");
        assert!(!removed.status.success(), "{removed:?}");
        let output = String::from_utf8(removed.stdout).unwrap();
        assert!(output.contains(&format!("{valid}\tavailable\t\n")));
        assert!(cache.content_path(&valid).is_file());
        let metadata = cache.inspect(&valid).unwrap().metadata;
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].name, "custom");
        for id in [corrupt, orphan] {
            let inspection = cache.inspect(&id).unwrap();
            assert!(!cache.content_path(&id).exists());
            assert!(inspection.metadata.is_empty());
        }
        assert_eq!(fs::read(external).unwrap(), b"broken content");
        assert_eq!(
            fs::read(invalid_child).unwrap(),
            b"do not recursively remove"
        );
    }
}

#[test]
fn remove_all_deletes_content_and_orphan_metadata_through_cache_inventory() {
    use mcpm_backend::local::cache::{
        CacheDir, CacheLinkPreference, codec::CacheDirMetadata, mods::ModsCacheDir,
    };

    let root = tempfile::tempdir().unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let content = cache.import(&fixture, CacheLinkPreference::Copy).unwrap();
    let orphan_fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dependency-problems/mods/fabric_only.jar");
    let orphan = cache
        .import(&orphan_fixture, CacheLinkPreference::Copy)
        .unwrap();
    CacheDir::new(cache.path())
        .unwrap()
        .write_metadata(&orphan, "custom", &CacheDirMetadata::new())
        .unwrap()
        .unwrap();
    fs::remove_file(cache.content_path(&orphan)).unwrap();
    assert_eq!(cache.scan().unwrap().len(), 2);

    let removed = batch(root.path(), cache.path(), &["remove", "--all"], "");
    assert!(removed.status.success(), "{removed:?}");
    let mut expected = [content, orphan];
    expected.sort();
    let rows: Vec<_> = String::from_utf8(removed.stdout)
        .unwrap()
        .lines()
        .map(|line| line.split('\t').next().unwrap().to_owned())
        .collect();
    assert_eq!(rows, expected.map(|id| id.to_string()));
    assert!(cache.scan().unwrap().is_empty());
    assert!(!cache.content_path(&content).exists());

    let empty = batch(root.path(), cache.path(), &["remove", "--all"], "");
    assert!(empty.status.success(), "{empty:?}");
    assert!(empty.stdout.is_empty());
    let mixed = batch(
        root.path(),
        cache.path(),
        &["remove", "--all"],
        &format!("{content}\n"),
    );
    assert!(!mixed.status.success());
}

#[test]
fn batches_preserve_input_order_and_completed_items_when_a_later_item_fails() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    assert!(
        run(root.path(), &cache, &["init".as_ref()])
            .status
            .success()
    );
    let first = root.path().join("first.jar");
    let second = root.path().join("second.jar.disabled");
    fs::write(&first, b"first content").unwrap();
    fs::write(&second, b"second content").unwrap();
    let imported = run(
        root.path(),
        &cache,
        &["import".as_ref(), first.as_os_str(), second.as_os_str()],
    );
    assert!(imported.status.success(), "{imported:?}");
    let imported = imports(&imported);
    assert_eq!(imported.len(), 2);
    assert_eq!(
        fs::read(imported[0]["data_path"].as_str().unwrap()).unwrap(),
        b"first content"
    );
    assert_eq!(
        fs::read(imported[1]["data_path"].as_str().unwrap()).unwrap(),
        b"second content"
    );
    let id = imported[0]["id"].as_str().unwrap();
    let first_copy = root.path().join("copy one.jar");
    let second_copy = root.path().join("copy two.jar");
    let copied = run(
        root.path(),
        &cache,
        &[
            "export".as_ref(),
            id.as_ref(),
            first_copy.as_os_str(),
            second_copy.as_os_str(),
        ],
    );
    assert!(copied.status.success(), "{copied:?}");
    assert_eq!(String::from_utf8_lossy(&copied.stdout).lines().count(), 2);
    assert_eq!(fs::read(&first_copy).unwrap(), b"first content");
    assert_eq!(fs::read(&second_copy).unwrap(), b"first content");
    let retained = root.path().join("retained.jar");
    let skipped = root.path().join("skipped.jar");
    let failed = run(
        root.path(),
        &cache,
        &[
            "export".as_ref(),
            id.as_ref(),
            retained.as_os_str(),
            first_copy.as_os_str(),
            skipped.as_os_str(),
        ],
    );
    assert!(!failed.status.success());
    assert_eq!(String::from_utf8_lossy(&failed.stdout).lines().count(), 1);
    assert_eq!(fs::read(&retained).unwrap(), b"first content");
    assert!(!skipped.exists());
    let missing = root.path().join("missing.jar");
    let partial = run(
        root.path(),
        &cache,
        &[
            "import".as_ref(),
            second.as_os_str(),
            missing.as_os_str(),
            first.as_os_str(),
        ],
    );
    assert!(!partial.status.success());
    let reused = imports(&partial);
    assert_eq!(reused.len(), 1);
    assert_eq!(reused[0]["id"], imported[1]["id"]);
    assert_eq!(reused[0]["published"], false);
    assert!(reused[0]["method"].is_null());
    let listed = run(root.path(), &cache, &["list".as_ref()]);
    assert!(listed.status.success());
    assert_eq!(String::from_utf8_lossy(&listed.stdout).lines().count(), 2);
    let protected_target = cache.join("unexpected.dat");
    let rejected = run(
        root.path(),
        &cache,
        &["export".as_ref(), id.as_ref(), protected_target.as_os_str()],
    );
    assert!(!rejected.status.success());
    assert!(!protected_target.exists());
}

/// Exercise identity reuse, duplicate paths, nested declarations and aggregation on the file host.
#[test]
fn remote_inspection_counts_full_partial_and_uncached_files_before_expanding_rows() {
    use mcpm_backend::local::cache::CacheLinkPreference;
    use mcpm_backend::local::cache::mods::MetadataRefresh;
    use mcpm_backend::local::cache::mods::ModsCacheDir;
    use mcpm_backend::local::modloader::BUILTIN_LOADERS;
    use serde_json::json;
    use std::io::Write;

    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    fs::create_dir(&mods).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let writer = &cache;
    let full = writer
        .import(
            &fixture,
            mcpm_backend::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    writer
        .refresh_metadata(full, &BUILTIN_LOADERS, MetadataRefresh::Force)
        .unwrap();
    writer
        .export_file(
            &(full),
            &mods.join("full.jar.disabled"),
            CacheLinkPreference::HardLink,
        )
        .unwrap();
    let partial_fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dependency-problems/mods/fabric_only.jar");
    let partial = writer
        .import(
            &partial_fixture,
            mcpm_backend::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    let fabric = BUILTIN_LOADERS
        .iter()
        .filter(|(id, _)| id.as_str() == "fabric")
        .map(|(id, loader)| (id.clone(), loader.clone()))
        .collect();
    writer
        .refresh_metadata(partial, &fabric, MetadataRefresh::Force)
        .unwrap();
    writer
        .export_file(
            &(partial),
            &mods.join("partial.jar"),
            CacheLinkPreference::HardLink,
        )
        .unwrap();
    let partial_digest = writer.mod_metadata(partial).digest_path().to_owned();
    let original_digest = fs::read(&partial_digest).unwrap();
    fs::copy(&fixture, mods.join("copy.jar")).unwrap();
    fs::write(mods.join("broken.jar"), b"invalid ZIP").unwrap();

    for command in ["mod.info", "mod.list"] {
        for enabled in [false, true] {
            let mut contents = if command == "mod.info" {
                json!({"directory":mods, "selection":{"queries":[
                    {"kind":"file", "value":"full.jar.disabled"},
                    {"kind":"file", "value":"partial.jar"},
                    {"kind":"file", "value":"copy.jar"},
                    {"kind":"file", "value":"broken.jar"},
                    {"kind":"file", "value":"full.jar.disabled"}
                ]}})
            } else {
                json!({"paths":[mods, mods.join("full.jar.disabled")]})
            };
            if enabled {
                contents["cache"] = json!({"kind":"directory", "path":cache.path()});
            }
            let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm"))
                .args(["--log-level", "off", "remote"])
                .current_dir(root.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            writeln!(child.stdin.take().unwrap(), "{}", json!({
                "guid":"00000000-0000-0000-0000-000000000001", "command":command, "contents":contents
            })).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let replies = rows(&output);
            let done = replies
                .iter()
                .find(|reply| reply["status"] == "data" && reply["contents"]["event"] == "done")
                .unwrap();
            assert_eq!(
                done["contents"]["cache"],
                json!({
                    "fully_cached":if enabled { 2 } else { 0 },
                    "partially_cached":usize::from(enabled), "total":4
                }),
                "{command}, enabled={enabled}: {replies:?}"
            );
            // Broken archives contribute to totals even though no ModMeta can be emitted.
            if command == "mod.info" {
                assert_eq!(done["contents"]["failed"], 1);
            } else {
                assert_eq!(done["contents"]["success"], false);
            }
        }
    }
    assert_eq!(
        fs::read(partial_digest).unwrap(),
        original_digest,
        "cache-enabled opens must not write missing loader results"
    );
    assert_eq!(cache.list_metadata().unwrap().len(), 2);
}
