//! Explicit mod-cache workflow: shared selectors, complete metadata, deduplication and partial failure.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use kako_mods_lib::local::{
    files::FileIdentity,
    mcmod::cache::{CacheStatus, CacheUnavailableReason, CachedIdentification, ModsCache},
    modloader::BUILTIN_LOADERS,
};
use serde_json::Value;

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn run(root: &Path, cache: &Path, arguments: &[&str], input: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm"));
    command
        .args(["--raw", "--log-level", "off", "mod", "cache", "--cache-dir"])
        .arg(cache)
        .args(["--contents", "metadata"])
        .args(arguments)
        .current_dir(root)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn setup() -> (tempfile::TempDir, ModsCache) {
    let root = tempfile::tempdir().unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    (root, cache)
}

#[test]
fn mixed_selectors_and_stdin_cache_whole_archives_once_without_linking_sources() {
    let (root, cache) = setup();
    let mods = root.path().join("mods");
    fs::create_dir(&mods).unwrap();
    fs::copy(
        fixture("dependency-problems/mods/fabric_only.jar"),
        mods.join("first.jar"),
    )
    .unwrap();
    fs::copy(
        fixture("complex-mod.jar"),
        mods.join("complex.jar.disabled"),
    )
    .unwrap();
    fs::copy(fixture("complex-mod.jar"), mods.join("duplicate.jar")).unwrap();
    let original_identity = FileIdentity::read(&mods.join("complex.jar.disabled")).unwrap();
    let original_permissions = fs::metadata(mods.join("complex.jar.disabled"))
        .unwrap()
        .permissions();
    // Explicit filename first, then nested ID, another auto filename, exact name and
    // stdin ID. Overlapping selections must not truncate the stored ModMeta tree.
    let output = run(
        root.path(),
        cache.root(),
        &[
            "--directory",
            "mods",
            "--file",
            "first.jar",
            "--modid",
            "omnibus_nested",
            "duplicate.jar",
            "--name",
            "Complex Mod / Fabric",
            "--parallelism",
            "3",
            "--input",
            "modid",
        ],
        Some("omnibus_leaf\n"),
    );
    assert!(output.status.success(), "{output:?}");
    let records = rows(&output);
    assert_eq!(records.len(), 3, "{records:?}");
    assert!(records[0]["path"].as_str().unwrap().ends_with("first.jar"));
    let complex = records
        .iter()
        .find(|record| {
            record["path"]
                .as_str()
                .unwrap()
                .ends_with("complex.jar.disabled")
        })
        .unwrap();
    let duplicate = records
        .iter()
        .find(|record| record["path"].as_str().unwrap().ends_with("duplicate.jar"))
        .unwrap();
    assert_eq!(complex["id"], duplicate["id"]);
    assert_eq!(
        records
            .iter()
            .filter(|record| record["published"] == true)
            .count(),
        2
    );
    assert!(records.iter().all(|record| record["status"] == "available"));
    let id = complex["id"].as_str().unwrap().parse().unwrap();
    let entry = cache.reader().unwrap().get(id);
    for loader in ["forge", "neoforge", "fabric"] {
        let CachedIdentification::Recognized(snapshot) =
            entry.digest().unwrap().identification(loader)
        else {
            panic!("expected complete {loader} metadata");
        };
        let restored = snapshot.to_mod_meta(&BUILTIN_LOADERS).unwrap();
        assert!(restored.mods.iter().any(|item| item.modid == "omnibus"));
        assert_eq!(restored.nested.len(), 3);
        assert!(!restored.nested[0].meta.nested.is_empty());
    }
    assert_eq!(
        FileIdentity::read(&mods.join("complex.jar.disabled")).unwrap(),
        original_identity
    );
    assert_ne!(
        FileIdentity::read(entry.data_path()).unwrap(),
        original_identity
    );
    assert_eq!(
        fs::metadata(mods.join("complex.jar.disabled"))
            .unwrap()
            .permissions(),
        original_permissions
    );
    assert_eq!(cache.reader().unwrap().list().unwrap().len(), 2);

    let repeated = run(
        root.path(),
        cache.root(),
        &["--directory", "mods", "--all"],
        None,
    );
    assert!(repeated.status.success(), "{repeated:?}");
    assert!(
        rows(&repeated)
            .iter()
            .all(|record| record["published"] == false && record["method"].is_null())
    );
}

#[test]
fn import_and_identification_failures_continue_and_unrecognized_remains_a_positive_result() {
    let (root, cache) = setup();
    fs::copy(fixture("complex-mod.jar"), root.path().join("good.jar")).unwrap();
    fs::write(root.path().join("broken.jar"), b"not a ZIP").unwrap();
    fs::write(root.path().join("wrong.txt"), b"not a mod filename").unwrap();
    // Minimal valid empty ZIP, successfully checked but unrecognized by every loader.
    fs::write(
        root.path().join("empty.jar"),
        [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    )
    .unwrap();
    let output = run(
        root.path(),
        cache.root(),
        &[
            "--file",
            "missing.jar",
            "--file",
            "broken.jar",
            "--file",
            "wrong.txt",
            "--file",
            "good.jar",
            "--file",
            "empty.jar",
            "--parallelism",
            "3",
        ],
        None,
    );
    assert!(!output.status.success());
    let records = rows(&output);
    assert_eq!(records.len(), 5, "{records:?}");
    assert_eq!(records[0]["failed_stage"], "import");
    assert!(records[0]["id"].is_null());
    assert_eq!(records[1]["failed_stage"], "metadata");
    assert_eq!(records[1]["published"], true);
    let broken_id = records[1]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        cache.reader().unwrap().get(broken_id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    assert_eq!(records[2]["failed_stage"], "import");
    assert!(records[2]["id"].is_null());
    assert_eq!(records[3]["success"], true);
    assert_eq!(records[4]["success"], true);
    let empty_id = records[4]["id"].as_str().unwrap().parse().unwrap();
    let empty = cache.reader().unwrap().get(empty_id);
    assert_eq!(empty.status(), &CacheStatus::Available);
    assert!(
        empty
            .digest()
            .unwrap()
            .identifications()
            .all(|(_, result)| matches!(result, CachedIdentification::Unrecognized))
    );
    assert_eq!(cache.reader().unwrap().list().unwrap().len(), 3);
}

#[test]
fn explicit_hard_link_and_metadata_completion_preserve_existing_identity_and_unselected_results() {
    let (root, cache) = setup();
    let source = root.path().join("source.jar");
    fs::copy(fixture("complex-mod.jar"), &source).unwrap();
    let identity = FileIdentity::read(&source).unwrap();
    let first = run(
        root.path(),
        cache.root(),
        &["source.jar", "--mode", "hard-link", "--loader", "fabric"],
        None,
    );
    assert!(first.status.success(), "{first:?}");
    let first = rows(&first);
    assert_eq!(first[0]["method"], "hard_link");
    let id = first[0]["id"].as_str().unwrap().parse().unwrap();
    let entry = cache.reader().unwrap().get(id);
    assert_eq!(FileIdentity::read(entry.data_path()).unwrap(), identity);
    assert!(matches!(
        entry.digest().unwrap().identification("forge"),
        CachedIdentification::Missing
    ));

    let second = run(
        root.path(),
        cache.root(),
        &["source.jar", "--loaders", "forge,neoforge"],
        None,
    );
    assert!(second.status.success(), "{second:?}");
    assert_eq!(rows(&second)[0]["published"], false);
    let force = run(
        root.path(),
        cache.root(),
        &["source.jar", "--loader", "fabric", "--force"],
        None,
    );
    assert!(force.status.success(), "{force:?}");
    let entry = cache.reader().unwrap().get(id);
    assert_eq!(FileIdentity::read(entry.data_path()).unwrap(), identity);
    for loader in ["forge", "neoforge", "fabric"] {
        assert!(matches!(
            entry.digest().unwrap().identification(loader),
            CachedIdentification::Recognized(_)
        ));
    }
    assert_eq!(cache.reader().unwrap().list().unwrap().len(), 1);
}
