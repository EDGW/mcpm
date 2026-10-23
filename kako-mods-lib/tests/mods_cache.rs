//! Content-addressed cache publication, concurrency, metadata validity and hard-link lifetime.

use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use kako_mods_lib::local::{
    mcmod::{
        ModEntry, ModMeta,
        cache::{
            CacheId, CacheStatus, CacheUnavailableReason, CachedIdentification, ExportMethod,
            ExportMode, ImportMethod, ImportMode, MetadataRefresh, ModsCache, VerificationMode,
        },
    },
    modloader::{BUILTIN_LOADERS, LoaderId, ModLoader, ModLoaderMeta},
};
use serde_json::{Value, json};

#[test]
fn imports_deduplicate_content_and_copy_snapshots_remain_independent() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.jar.disabled");
    fs::write(&source, b"original bytes").unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut access = cache.writer().unwrap();
    let first = access.import(&source).unwrap();
    let second = access.import(&source).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.to_string().len(), 64);
    assert_eq!(
        first.to_string(),
        blake3::hash(b"original bytes").to_hex().as_str()
    );
    fs::write(&source, b"changed bytes").unwrap();
    let entry = access.get(first);
    assert_eq!(fs::read(entry.data_path()).unwrap(), b"original bytes");
    assert_eq!(
        fs::read(access.get(second).data_path()).unwrap(),
        b"original bytes"
    );
    assert_eq!(
        entry.data_path().parent().unwrap().file_name().unwrap(),
        &first.to_string()[..2]
    );
    assert!(entry.digest().is_none());
    assert!(!entry.digest_path().exists());
    assert_eq!(
        access.verify(first, VerificationMode::Content),
        CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    assert_eq!(access.list().unwrap().len(), 1);
}

#[test]
fn status_distinguishes_orphaned_halves_invalid_versions_and_same_size_corruption() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input");
    let bytes = archive(&[]);
    fs::write(&source, &bytes).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut access = cache.writer().unwrap();
    let id = access.import(&source).unwrap();
    let entry = access
        .refresh_metadata(id, &selected(&["forge"]), MetadataRefresh::Force)
        .unwrap();
    let descriptor = fs::read(entry.digest_path()).unwrap();
    fs::remove_file(entry.digest_path()).unwrap();
    assert_eq!(
        access.get(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    assert_eq!(access.list().unwrap()[0].id(), id);
    for invalid in [
        b"bad json".to_vec(),
        {
            let mut value: Value = serde_json::from_slice(&descriptor).unwrap();
            value["version"] = 999.into();
            serde_json::to_vec(&value).unwrap()
        },
        {
            let mut value: Value = serde_json::from_slice(&descriptor).unwrap();
            value.as_object_mut().unwrap().remove("sha256");
            serde_json::to_vec(&value).unwrap()
        },
        {
            let mut value: Value = serde_json::from_slice(&descriptor).unwrap();
            value["loaders"] = json!({});
            serde_json::to_vec(&value).unwrap()
        },
    ] {
        fs::write(entry.digest_path(), invalid).unwrap();
        assert_eq!(
            access.get(id).status(),
            &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
        );
    }
    fs::write(entry.digest_path(), &descriptor).unwrap();
    // Replace the name rather than writing through a protected file handle.
    fs::remove_file(entry.data_path()).unwrap();
    assert_eq!(
        access.get(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    fs::write(entry.data_path(), vec![b'b'; bytes.len()]).unwrap();
    assert_eq!(
        access.verify(id, VerificationMode::Structure),
        CacheStatus::Available
    );
    assert_eq!(
        access.verify(id, VerificationMode::Content),
        CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    let destination = root.path().join("output.jar");
    assert!(access.export(id, &destination, ExportMode::Copy).is_err());
    assert!(!destination.exists());
    assert!(access.remove(id).unwrap());
    assert_eq!(access.get(id).status(), &CacheStatus::NotFound);
    assert!(!access.remove(id).unwrap());
}

#[test]
fn nested_metadata_roundtrips_and_loader_updates_publish_all_or_nothing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("mods.jar");
    let child = archive(&[(
        "fabric.mod.json",
        json!({"schemaVersion":1,"id":"child","version":"2","name":"Child"})
            .to_string()
            .into_bytes(),
    )]);
    let outer = archive(&[("fabric.mod.json", json!({"schemaVersion":1,"id":"outer","version":"1","jars":[{"file":"META-INF/jars/child.jar"}],"depends":{"minecraft":">=1.20"}}).to_string().into_bytes()),
        ("META-INF/jars/child.jar", child)]);
    fs::write(&source, outer).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut access = cache.writer().unwrap();
    let id = access.import(&source).unwrap();
    let fabric = selected(&["fabric"]);
    let entry = access
        .refresh_metadata(id, &fabric, MetadataRefresh::Force)
        .unwrap();
    let CachedIdentification::Recognized(snapshot) =
        entry.digest().unwrap().identification("fabric")
    else {
        panic!("expected recognized Fabric metadata");
    };
    let restored = snapshot.to_mod_meta(&fabric).unwrap();
    assert_eq!(restored.mods[0].modid, "outer");
    assert_eq!(restored.nested[0].path, "META-INF/jars/child.jar");
    assert_eq!(restored.nested[0].meta.mods[0].modid, "child");
    assert!(!restored.mods[0].dependencies.is_empty());
    assert!(snapshot.to_mod_meta(&HashMap::new()).is_err());
    let original = fs::read(entry.digest_path()).unwrap();
    let mut failing = selected(&["forge"]);
    failing.insert(FAILURE.id().clone(), FAILURE.clone());
    assert!(
        access
            .refresh_metadata(id, &failing, MetadataRefresh::Force)
            .is_err()
    );
    assert_eq!(fs::read(entry.digest_path()).unwrap(), original);
    let entry = access
        .refresh_metadata(id, &selected(&["forge"]), MetadataRefresh::Force)
        .unwrap();
    assert!(matches!(
        entry.digest().unwrap().identification("forge"),
        CachedIdentification::Unrecognized
    ));
    assert!(matches!(
        entry.digest().unwrap().identification("fabric"),
        CachedIdentification::Recognized(_)
    ));
    // A parser-version change invalidates previous results, not the stored bytes.
    let mut outdated: Value =
        serde_json::from_slice(&fs::read(entry.digest_path()).unwrap()).unwrap();
    outdated["metadata_version"] = 0.into();
    fs::write(entry.digest_path(), serde_json::to_vec(&outdated).unwrap()).unwrap();
    assert_eq!(
        access.get(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    let fresh = access
        .refresh_metadata(id, &fabric, MetadataRefresh::MissingOrOutdated)
        .unwrap();
    assert_eq!(fresh.status(), &CacheStatus::Available);
    assert_eq!(fresh.digest().unwrap().identifications().len(), 1);
}

#[test]
fn unrecognized_is_a_cached_result_not_a_missing_digest() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.jar");
    fs::write(&source, archive(&[])).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let id = writer.import(&source).unwrap();
    let missing = writer.get(id);
    assert_eq!(
        missing.status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    // No result is invented when no loaders are selected.
    writer
        .refresh_metadata(id, &HashMap::new(), MetadataRefresh::Force)
        .unwrap();
    assert!(!missing.digest_path().exists());
    writer
        .export(
            id,
            &root.path().join("without-digest.jar"),
            ExportMode::Copy,
        )
        .unwrap();
    let failing = [(FAILURE.id().clone(), FAILURE.clone())]
        .into_iter()
        .collect();
    assert!(
        writer
            .refresh_metadata(id, &failing, MetadataRefresh::Force)
            .is_err()
    );
    assert!(!missing.digest_path().exists());
    let loaders = [(UNRECOGNIZED.id().clone(), UNRECOGNIZED.clone())]
        .into_iter()
        .collect();
    let recognized = writer
        .refresh_metadata(id, &loaders, MetadataRefresh::MissingOrOutdated)
        .unwrap();
    assert_eq!(recognized.status(), &CacheStatus::Available);
    assert!(matches!(
        recognized.digest().unwrap().identification("unrecognized"),
        CachedIdentification::Unrecognized
    ));
    assert_eq!(UNRECOGNIZED_CALLS.load(Ordering::SeqCst), 1);
    let saved = fs::read(recognized.digest_path()).unwrap();
    writer
        .refresh_metadata(id, &loaders, MetadataRefresh::MissingOrOutdated)
        .unwrap();
    assert_eq!(UNRECOGNIZED_CALLS.load(Ordering::SeqCst), 1);
    writer
        .refresh_metadata(id, &loaders, MetadataRefresh::Force)
        .unwrap();
    assert_eq!(UNRECOGNIZED_CALLS.load(Ordering::SeqCst), 2);
    assert!(
        writer
            .refresh_metadata(id, &failing, MetadataRefresh::Force)
            .is_err()
    );
    assert_eq!(fs::read(recognized.digest_path()).unwrap(), saved);
    assert_eq!(
        writer.verify(id, VerificationMode::Content),
        CacheStatus::Available
    );
    fs::remove_file(recognized.digest_path()).unwrap();
    assert_eq!(
        writer.get(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    writer
        .refresh_metadata(id, &loaders, MetadataRefresh::MissingOrOutdated)
        .unwrap();
    assert_eq!(UNRECOGNIZED_CALLS.load(Ordering::SeqCst), 3);
}

#[test]
fn protected_hard_links_survive_cache_deletion_and_can_be_replaced() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.jar");
    fs::write(&source, b"unchanged").unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut access = cache.writer().unwrap();
    let id = access.import(&source).unwrap();
    let entry = access.get(id);
    let linked = root.path().join("linked.jar");
    let copied = root.path().join("copied.jar");
    assert_eq!(
        access.export(id, &linked, ExportMode::HardLink).unwrap(),
        ExportMethod::HardLink
    );
    assert_eq!(
        access.export(id, &copied, ExportMode::Copy).unwrap(),
        ExportMethod::Copy
    );
    assert!(
        access
            .export(id, &linked, ExportMode::PreferHardLink)
            .is_err()
    );
    for mode in [
        ExportMode::Copy,
        ExportMode::HardLink,
        ExportMode::PreferHardLink,
    ] {
        let internal = entry.data_path().parent().unwrap().join("unexpected.dat");
        assert!(matches!(
            access.export(id, &internal, mode),
            Err(kako_mods_lib::local::mcmod::cache::CacheError::ExportIntoCache(_))
        ));
        assert!(!internal.exists());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        assert_eq!(
            fs::metadata(&linked).unwrap().ino(),
            fs::metadata(entry.data_path()).unwrap().ino()
        );
        assert_eq!(
            fs::metadata(&linked).unwrap().permissions().mode() & 0o222,
            0
        );
    }
    #[cfg(windows)]
    {
        assert!(!fs::metadata(&linked).unwrap().permissions().readonly());
        assert!(fs::OpenOptions::new().write(true).open(&linked).is_err());
        assert!(fs::OpenOptions::new().append(true).open(&linked).is_err());
    }
    fs::write(&copied, b"independent").unwrap();
    assert_eq!(fs::read(entry.data_path()).unwrap(), b"unchanged");
    assert!(access.remove(id).unwrap());
    assert_eq!(fs::read(&linked).unwrap(), b"unchanged");
    let mut replacement = tempfile::NamedTempFile::new_in(root.path()).unwrap();
    replacement.write_all(b"replacement").unwrap();
    replacement.persist(&linked).unwrap();
    assert_eq!(fs::read(&linked).unwrap(), b"replacement");
}

#[test]
fn hard_link_import_protects_source_and_copy_import_remains_independent() {
    use kako_mods_lib::local::files::FileIdentity;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.jar");
    fs::write(&source, b"shared content").unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    for mode in [
        ImportMode::Copy,
        ImportMode::HardLink,
        ImportMode::PreferHardLink,
    ] {
        let outcome = writer.import_with_mode(&source, mode).unwrap();
        let entry = writer.get(outcome.id);
        assert!(outcome.published);
        assert_eq!(
            outcome.method,
            Some(if mode == ImportMode::Copy {
                ImportMethod::Copy
            } else {
                ImportMethod::HardLink
            })
        );
        assert_eq!(
            FileIdentity::read(&source).unwrap() == FileIdentity::read(entry.data_path()).unwrap(),
            mode != ImportMode::Copy
        );
        assert_eq!(
            entry.status(),
            &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
        );
        assert!(writer.remove(outcome.id).unwrap());
        assert_eq!(fs::read(&source).unwrap(), b"shared content");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&source).unwrap().permissions().mode() & 0o222,
            0
        );
    }
    #[cfg(windows)]
    {
        assert!(fs::OpenOptions::new().write(true).open(&source).is_err());
        assert!(fs::OpenOptions::new().append(true).open(&source).is_err());
    }
    fs::remove_file(&source).unwrap();
    fs::write(&source, b"replacement").unwrap();
    assert_eq!(fs::read(source).unwrap(), b"replacement");
}

#[test]
fn duplicate_hard_link_import_reuses_copy_without_connecting_or_protecting_source() {
    use kako_mods_lib::local::files::FileIdentity;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.jar");
    fs::write(&source, b"duplicate content").unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let original = writer.import_with_mode(&source, ImportMode::Copy).unwrap();
    let data = writer.get(original.id).data_path().to_owned();
    let cached_identity = FileIdentity::read(&data).unwrap();
    let source_identity = FileIdentity::read(&source).unwrap();
    assert_ne!(source_identity, cached_identity);
    #[cfg(unix)]
    let source_mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&source).unwrap().permissions().mode()
    };
    for mode in [
        ImportMode::Copy,
        ImportMode::HardLink,
        ImportMode::PreferHardLink,
    ] {
        let duplicate = writer.import_with_mode(&source, mode).unwrap();
        assert_eq!(duplicate.id, original.id);
        assert!(!duplicate.published);
        assert_eq!(duplicate.method, None);
        assert_eq!(FileIdentity::read(&source).unwrap(), source_identity);
        assert_eq!(FileIdentity::read(&data).unwrap(), cached_identity);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&source).unwrap().permissions().mode(),
            source_mode
        );
    }
    // This must remain writable, including with the Windows content ACL helper.
    fs::write(&source, b"independent update").unwrap();
    assert_eq!(fs::read(data).unwrap(), b"duplicate content");
}

#[test]
fn readers_and_writers_can_coexist_without_transaction_locks() {
    let root = tempfile::tempdir().unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"snapshot").unwrap();
    let reader = cache.reader().unwrap();
    let mut first = cache.writer().unwrap();
    let mut second = cache.writer().unwrap();
    let imported = first.import_with_mode(&source, ImportMode::Copy).unwrap();
    assert!(imported.published);
    let duplicate = second
        .import_with_mode(&source, ImportMode::HardLink)
        .unwrap();
    assert_eq!(duplicate.id, imported.id);
    assert!(!duplicate.published);
    assert_eq!(reader.list().unwrap().len(), 1);
    assert!(second.remove(imported.id).unwrap());
    assert_eq!(first.get(imported.id).status(), &CacheStatus::NotFound);
    assert!(reader.list().unwrap().is_empty());
    assert!(!cache.root().join(".lock").exists());
}

#[test]
fn cache_process_worker() {
    use kako_mods_lib::local::files::FileIdentity;
    use std::time::{Duration, Instant};
    let Some(root) = std::env::var_os("KAKO_CACHE_WORKER_ROOT") else {
        return;
    };
    let marker = std::env::var_os("KAKO_CACHE_WORKER_MARKER").unwrap();
    let start = std::env::var_os("KAKO_CACHE_WORKER_START").unwrap();
    let cache = ModsCache::new(root);
    let mut writer = cache.writer().unwrap();
    fs::write(Path::new(&marker).with_extension("ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !Path::new(&start).exists() {
        assert!(Instant::now() < deadline, "parent did not release workers");
        std::thread::sleep(Duration::from_millis(5));
    }
    let outcome = writer
        .import_with_mode(
            Path::new(&std::env::var_os("KAKO_CACHE_WORKER_SOURCE").unwrap()),
            ImportMode::Copy,
        )
        .unwrap();
    let entry = writer.get(outcome.id);
    let identity = FileIdentity::read(entry.data_path()).unwrap();
    fs::write(
        marker,
        format!("{}\n{}\n{identity:?}\n", outcome.id, outcome.published),
    )
    .unwrap();
}

#[test]
fn simultaneous_process_imports_publish_one_object_without_replacing_its_identity() {
    use kako_mods_lib::local::files::FileIdentity;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let source = root.path().join("source");
    fs::write(&source, vec![42; 1024 * 1024]).unwrap();
    let start = root.path().join("start");
    // Holding both capabilities must not prevent another process from publishing.
    let _reader = cache.reader().unwrap();
    let _writer = cache.writer().unwrap();
    let mut children = Vec::new();
    for index in 0..4 {
        let marker = root.path().join(format!("child-{index}"));
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cache_process_worker"])
            .env("KAKO_CACHE_WORKER_ROOT", cache.root())
            .env("KAKO_CACHE_WORKER_SOURCE", &source)
            .env("KAKO_CACHE_WORKER_MARKER", &marker)
            .env("KAKO_CACHE_WORKER_START", &start)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        children.push((child, marker));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while children
        .iter()
        .any(|(_, marker)| !marker.with_extension("ready").exists())
    {
        assert!(Instant::now() < deadline, "workers did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    fs::write(start, b"start").unwrap();
    let mut results = Vec::new();
    for (mut child, marker) in children {
        assert!(child.wait().unwrap().success());
        let result = fs::read_to_string(marker).unwrap();
        let fields: Vec<_> = result.lines().map(str::to_owned).collect();
        results.push(fields);
    }
    let id: CacheId = results[0][0].parse().unwrap();
    let entry = cache.reader().unwrap().get(id);
    let identity = format!("{:?}", FileIdentity::read(entry.data_path()).unwrap());
    assert_eq!(
        results.iter().filter(|fields| fields[1] == "true").count(),
        1
    );
    assert!(
        results
            .iter()
            .all(|fields| fields[0] == id.to_string() && fields[2] == identity)
    );
    assert_eq!(cache.reader().unwrap().list().unwrap().len(), 1);
    assert_eq!(
        fs::read(entry.data_path()).unwrap(),
        fs::read(source).unwrap()
    );
}

fn archive(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn selected(ids: &[&str]) -> HashMap<LoaderId, ModLoaderMeta> {
    ids.iter()
        .map(|id| {
            let loader = BUILTIN_LOADERS.get(*id).unwrap();
            (loader.id().clone(), loader.clone())
        })
        .collect()
}

static FAILURE: LazyLock<ModLoaderMeta> = LazyLock::new(|| {
    ModLoaderMeta::new(
        "zz_failure".into(),
        "Failure".into(),
        Arc::new(FailingLoader),
    )
});

#[derive(Debug)]
struct FailingLoader;

impl ModLoader for FailingLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &FAILURE
    }
    fn try_identify(&self, _entry: &ModEntry) -> anyhow::Result<Option<ModMeta>> {
        anyhow::bail!("deliberate identification failure")
    }
}

static UNRECOGNIZED_CALLS: AtomicUsize = AtomicUsize::new(0);
static UNRECOGNIZED: LazyLock<ModLoaderMeta> = LazyLock::new(|| {
    ModLoaderMeta::new(
        "unrecognized".into(),
        "Unrecognized".into(),
        Arc::new(UnrecognizedLoader),
    )
});

#[derive(Debug)]
struct UnrecognizedLoader;

impl ModLoader for UnrecognizedLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &UNRECOGNIZED
    }
    fn try_identify(&self, _entry: &ModEntry) -> anyhow::Result<Option<ModMeta>> {
        UNRECOGNIZED_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}
