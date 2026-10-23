//! Batch cache CLI ownership: ordered imports, repeated exports and partial failure.

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
    let imported = rows(&imported);
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
    assert_eq!(rows(&copied).len(), 2);
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
    assert_eq!(rows(&failed).len(), 1);
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
    let reused = rows(&partial);
    assert_eq!(reused.len(), 1);
    assert_eq!(reused[0]["id"], imported[1]["id"]);
    assert_eq!(reused[0]["published"], false);
    assert!(reused[0]["method"].is_null());
    let listed = run(root.path(), &cache, &["list".as_ref()]);
    assert!(listed.status.success());
    assert_eq!(rows(&listed).len(), 2);
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
    use kako_mods_lib::local::{
        mcmod::cache::{ExportMode, MetadataRefresh, ModsCache},
        modloader::BUILTIN_LOADERS,
    };
    use serde_json::json;
    use std::io::Write;

    let root = tempfile::tempdir().unwrap();
    let mods = root.path().join("mods");
    fs::create_dir(&mods).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let full = writer.import(&fixture).unwrap();
    writer
        .refresh_metadata(full, &BUILTIN_LOADERS, MetadataRefresh::Force)
        .unwrap();
    writer
        .export(full, &mods.join("full.jar.disabled"), ExportMode::HardLink)
        .unwrap();
    let partial_fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dependency-problems/mods/fabric_only.jar");
    let partial = writer.import(&partial_fixture).unwrap();
    let fabric = BUILTIN_LOADERS
        .iter()
        .filter(|(id, _)| id.as_str() == "fabric")
        .map(|(id, loader)| (id.clone(), loader.clone()))
        .collect();
    writer
        .refresh_metadata(partial, &fabric, MetadataRefresh::Force)
        .unwrap();
    writer
        .export(partial, &mods.join("partial.jar"), ExportMode::HardLink)
        .unwrap();
    let partial_digest = writer.get(partial).digest_path().to_owned();
    let original_digest = fs::read(&partial_digest).unwrap();
    drop(writer);
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
                contents["cache"] = json!({"kind":"directory", "path":cache.root()});
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
    assert_eq!(cache.reader().unwrap().list().unwrap().len(), 2);
}
