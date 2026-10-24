//! Cache-aware opening: loader bypass, partial results, stale identities and directory ownership.

use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use kako_mods_lib::local::cache::CacheLinkPreference;
use kako_mods_lib::local::cache::mods::MetadataRefresh;
use kako_mods_lib::local::cache::mods::ModsCacheDir;
use kako_mods_lib::local::mcmod;
use kako_mods_lib::local::mcmod::CachedModMeta;
use kako_mods_lib::local::mcmod::IdentificationErrorAction;
use kako_mods_lib::local::mcmod::LoaderCacheMissReason;
use kako_mods_lib::local::mcmod::LoaderCacheUsage;
use kako_mods_lib::local::mcmod::ModEntry;
use kako_mods_lib::local::mcmod::ModMeta;
use kako_mods_lib::local::mcmod::ModOpenCache;
use kako_mods_lib::local::mcmod::ModOpenCacheReport;
use kako_mods_lib::local::mcmod::ModOpenContext;
use kako_mods_lib::local::modloader::BUILTIN_LOADERS;
use kako_mods_lib::local::modloader::LoaderId;
use kako_mods_lib::local::modloader::ModLoader;
use kako_mods_lib::local::modloader::ModLoaderMeta;

#[derive(Debug)]
struct CountingLoader {
    inner: ModLoaderMeta,
    calls: Arc<AtomicUsize>,
}
impl ModLoader for CountingLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &self.inner
    }
    fn try_identify(&self, entry: &ModEntry) -> anyhow::Result<Option<ModMeta>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.instance().try_identify(entry)
    }
}

fn loaders() -> (
    HashMap<LoaderId, ModLoaderMeta>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let fabric_calls = Arc::new(AtomicUsize::new(0));
    let forge_calls = Arc::new(AtomicUsize::new(0));
    let loaders = [("fabric", &fabric_calls), ("forge", &forge_calls)]
        .into_iter()
        .map(|(id, calls)| {
            let inner = BUILTIN_LOADERS[id].clone();
            let loader = ModLoaderMeta::new(
                inner.id().clone(),
                inner.name().to_owned(),
                Arc::new(CountingLoader {
                    inner,
                    calls: calls.clone(),
                }),
            );
            (loader.id().clone(), loader)
        })
        .collect();
    (loaders, fabric_calls, forge_calls)
}

fn fixture(root: &Path) -> (ModsCacheDir, std::path::PathBuf) {
    let cache = ModsCacheDir::initialize(root.join("cache")).unwrap();
    let source = root.join("example.jar");
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(br#"{"schemaVersion":1,"id":"example","version":"1"}"#)
        .unwrap();
    fs::write(&source, archive.finish().unwrap().into_inner()).unwrap();
    (cache, source)
}

fn context(cache: &ModsCacheDir) -> ModOpenContext {
    ModOpenContext::new(Some(ModOpenCache::Directory {
        path: cache.path().to_owned(),
    }))
    .unwrap()
}

fn open(
    path: &Path,
    loaders: &HashMap<LoaderId, ModLoaderMeta>,
    context: &ModOpenContext,
) -> mcmod::ModOpenReport {
    mcmod::open(
        ModEntry::new(path.to_owned()).unwrap(),
        loaders.clone(),
        context,
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .unwrap()
}

#[test]
fn recognized_and_null_results_bypass_loaders_and_preserve_input_identity() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let (loaders, fabric_calls, forge_calls) = loaders();
    let writer = &cache;
    let id = writer
        .import(
            &source,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    writer
        .refresh_metadata(id, &loaders, MetadataRefresh::Force)
        .unwrap();
    let linked = root.path().join("renamed.jar.disabled");
    writer
        .export_file(&(id), &linked, CacheLinkPreference::HardLink)
        .unwrap();
    fabric_calls.store(0, Ordering::SeqCst);
    forge_calls.store(0, Ordering::SeqCst);
    let context = context(&cache);
    let report = open(&linked, &loaders, &context);
    assert_eq!(report.cache.reuse(), mcmod::ModCacheReuse::Full);
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 0);
    assert_eq!(forge_calls.load(Ordering::SeqCst), 0);
    assert_eq!(report.file.entry.path(), linked);
    assert!(matches!(
        report.file.entry.status(),
        mcmod::ModStatus::Disabled
    ));
    assert_eq!(report.file.metadata.len(), 1);
    assert_eq!(report.file.metadata[0].mods[0].modid, "example");
    match report.cache {
        ModOpenCacheReport::Matched {
            id: matched,
            loaders,
            ..
        } => {
            assert_eq!(matched, id);
            assert_eq!(loaders["fabric"], LoaderCacheUsage::Recognized);
            assert_eq!(loaders["forge"], LoaderCacheUsage::Unrecognized);
        }
        other => panic!("unexpected report: {other:?}"),
    }
    let cached = CachedModMeta::try_from(&report.file.metadata[0]).unwrap();
    let restored = cached.to_mod_meta(&loaders).unwrap();
    assert_eq!(restored.mods[0].modid, "example");
    // Direct .dat paths also match, while preserving the separately supplied name.
    let data = cache.mod_metadata(id).data_path().to_owned();
    let entry = ModEntry::from_parts(data, mcmod::ModEntryName::new("cache.jar").unwrap());
    let direct = mcmod::open(entry, loaders, &context, |_, _, _, _| {
        IdentificationErrorAction::Abort
    })
    .unwrap();
    assert!(
        matches!(direct.cache, ModOpenCacheReport::Matched { id: matched, .. } if matched == id)
    );
}

#[test]
fn missing_loader_is_identified_but_cached_null_is_not() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let (loaders, fabric_calls, forge_calls) = loaders();
    let writer = &cache;
    let id = writer
        .import(
            &source,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    let forge_only = [(LoaderId::from("forge"), loaders["forge"].clone())]
        .into_iter()
        .collect();
    let entry = writer
        .refresh_metadata(id, &forge_only, MetadataRefresh::Force)
        .unwrap();
    let saved = fs::read(entry.digest_path()).unwrap();
    let linked = root.path().join("linked.jar");
    writer
        .export_file(&(id), &linked, CacheLinkPreference::HardLink)
        .unwrap();
    forge_calls.store(0, Ordering::SeqCst);
    let report = open(&linked, &loaders, &context(&cache));
    assert_eq!(report.cache.reuse(), mcmod::ModCacheReuse::Partial);
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 1);
    assert_eq!(forge_calls.load(Ordering::SeqCst), 0);
    assert_eq!(report.file.metadata.len(), 1);
    match report.cache {
        ModOpenCacheReport::Matched { loaders, .. } => {
            assert_eq!(
                loaders["fabric"],
                LoaderCacheUsage::Miss(LoaderCacheMissReason::Missing)
            );
            assert_eq!(loaders["forge"], LoaderCacheUsage::Unrecognized);
        }
        other => panic!("unexpected report: {other:?}"),
    }
    assert_eq!(fs::read(entry.digest_path()).unwrap(), saved);
    let repeated = open(&linked, &loaders, &context(&cache));
    assert_eq!(repeated.cache.reuse(), mcmod::ModCacheReuse::Partial);
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn cache_misses_never_import_files_or_change_the_source() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let (loaders, fabric_calls, forge_calls) = loaders();
    let identity = kako_mods_lib::local::files::FileIdentity::read(&source).unwrap();
    let contents = fs::read(&source).unwrap();
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&source).unwrap().permissions().mode()
    };
    let context = context(&cache);
    for _ in 0..2 {
        let report = open(&source, &loaders, &context);
        assert_eq!(report.file.metadata[0].mods[0].modid, "example");
        assert_eq!(report.cache.reuse(), mcmod::ModCacheReuse::None);
        assert!(cache.list_metadata().unwrap().is_empty());
    }
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 2);
    assert_eq!(forge_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        kako_mods_lib::local::files::FileIdentity::read(&source).unwrap(),
        identity
    );
    assert_eq!(fs::read(&source).unwrap(), contents);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&source).unwrap().permissions().mode(), mode);
    }
}

#[derive(Debug)]
struct ConcurrentLoader {
    inner: ModLoaderMeta,
    barrier: Arc<std::sync::Barrier>,
}

impl ModLoader for ConcurrentLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &self.inner
    }
    fn try_identify(&self, entry: &ModEntry) -> anyhow::Result<Option<ModMeta>> {
        self.barrier.wait();
        self.inner.instance().try_identify(entry)
    }
}

#[test]
fn concurrent_opens_do_not_create_cache_objects() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let alias = root.path().join("alias.jar");
    fs::hard_link(&source, &alias).unwrap();
    let contexts = [context(&cache), context(&cache)];
    let inner = BUILTIN_LOADERS["fabric"].clone();
    let loader = ModLoaderMeta::new(
        inner.id().clone(),
        inner.name().to_owned(),
        Arc::new(ConcurrentLoader {
            inner,
            barrier: Arc::new(std::sync::Barrier::new(2)),
        }),
    );
    let loaders = [(loader.id().clone(), loader)].into_iter().collect();
    let reports = std::thread::scope(|scope| {
        let a = scope.spawn(|| open(&source, &loaders, &contexts[0]));
        let b = scope.spawn(|| open(&alias, &loaders, &contexts[1]));
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert!(cache.list_metadata().unwrap().is_empty());
    assert!(
        reports
            .iter()
            .all(|r| r.cache.reuse() == mcmod::ModCacheReuse::None)
    );
}

#[derive(Debug)]
struct FailingLoader {
    inner: ModLoaderMeta,
}

impl ModLoader for FailingLoader {
    fn metadata(&self) -> &ModLoaderMeta {
        &self.inner
    }
    fn try_identify(&self, _entry: &ModEntry) -> anyhow::Result<Option<ModMeta>> {
        anyhow::bail!("deliberate loader failure")
    }
}

#[test]
fn skipped_and_aborted_loader_errors_do_not_publish() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let context = context(&cache);
    let inner = BUILTIN_LOADERS["forge"].clone();
    let failure = ModLoaderMeta::new(
        inner.id().clone(),
        inner.name().to_owned(),
        Arc::new(FailingLoader { inner }),
    );
    let loaders = [
        (failure.id().clone(), failure),
        ("fabric".into(), BUILTIN_LOADERS["fabric"].clone()),
    ]
    .into_iter()
    .collect::<HashMap<_, _>>();
    assert!(
        mcmod::open(
            ModEntry::new(source.clone()).unwrap(),
            loaders.clone(),
            &context,
            |_, _, _, _| IdentificationErrorAction::Abort
        )
        .is_err()
    );
    assert!(cache.list_metadata().unwrap().is_empty());
    let report = mcmod::open(
        ModEntry::new(source.clone()).unwrap(),
        loaders,
        &context,
        |_, _, _, _| IdentificationErrorAction::Skip,
    )
    .unwrap();
    assert_eq!(report.file.metadata[0].mods[0].modid, "example");
    assert!(cache.list_metadata().unwrap().is_empty());
}

#[test]
fn independent_copies_reuse_content_without_becoming_hard_links() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let (loaders, fabric_calls, _) = loaders();
    let writer = &cache;
    let id = writer
        .import(
            &source,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    writer
        .refresh_metadata(id, &loaders, MetadataRefresh::Force)
        .unwrap();
    let data = writer.mod_metadata(id).data_path().to_owned();
    let copy = root.path().join("copy.jar");
    fs::copy(&source, &copy).unwrap();
    let copy_identity = kako_mods_lib::local::files::FileIdentity::read(&copy).unwrap();
    assert_ne!(
        copy_identity,
        kako_mods_lib::local::files::FileIdentity::read(&data).unwrap()
    );
    fabric_calls.store(0, Ordering::SeqCst);
    let context = context(&cache);
    let report = open(&copy, &loaders, &context);
    assert_eq!(report.cache.reuse(), mcmod::ModCacheReuse::Full);
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        kako_mods_lib::local::files::FileIdentity::read(&copy).unwrap(),
        copy_identity
    );
    cache.remove(&(id)).unwrap();
    let report = open(&copy, &loaders, &context);
    assert!(matches!(report.cache, ModOpenCacheReport::Miss { .. }));
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 1);
    assert!(cache.list_metadata().unwrap().is_empty());
}

#[test]
fn missing_and_corrupt_digests_are_not_written_by_open() {
    let root = tempfile::tempdir().unwrap();
    let (cache, source) = fixture(root.path());
    let (loaders, fabric_calls, _) = loaders();
    let writer = &cache;
    let id = writer
        .import(
            &source,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    let entry = writer.mod_metadata(id);
    let linked = root.path().join("linked.jar");
    writer
        .export_file(&(id), &linked, CacheLinkPreference::HardLink)
        .unwrap();
    let context = context(&cache);
    for corrupt in [None, Some(b"invalid JSON".as_slice())] {
        if let Some(bytes) = corrupt {
            fs::write(entry.digest_path(), bytes).unwrap();
        }
        let report = open(&linked, &loaders, &context);
        assert_eq!(report.file.metadata[0].mods[0].modid, "example");
        match report.cache {
            ModOpenCacheReport::Matched { loaders, .. } => {
                assert_eq!(
                    loaders["fabric"],
                    LoaderCacheUsage::Miss(LoaderCacheMissReason::Unavailable)
                );
            }
            other => panic!("unexpected report: {other:?}"),
        }
        if let Some(bytes) = corrupt {
            assert_eq!(fs::read(entry.digest_path()).unwrap(), bytes);
        } else {
            assert!(!entry.digest_path().exists());
        }
    }
    assert_eq!(fabric_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn explicit_roots_require_manifest_and_never_adopt_foreign_or_legacy_data() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cache");
    assert!(ModsCacheDir::new(&path).is_err());
    assert!(!path.exists());
    fs::create_dir(&path).unwrap();
    assert!(ModsCacheDir::new(&path).is_err());
    fs::write(path.join("foreign"), b"data").unwrap();
    assert!(ModsCacheDir::initialize(&path).is_err());
    assert_eq!(fs::read(path.join("foreign")).unwrap(), b"data");
    fs::remove_file(path.join("foreign")).unwrap();
    let cache = ModsCacheDir::initialize(&path).unwrap();
    let manifest = cache.metadata_path();
    fs::write(&manifest, br#"{"version":99}"#).unwrap();
    assert!(ModsCacheDir::new(&path).is_err());
    assert!(ModsCacheDir::initialize(&path).is_err());
    fs::write(&manifest, b"broken").unwrap();
    assert!(ModsCacheDir::new(&path).is_err());
}

#[test]
fn concurrent_initialization_publishes_one_valid_directory() {
    let root = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let barrier = Arc::new(std::sync::Barrier::new(8));
        for _ in 0..8 {
            let path = root.path().join("cache");
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                let cache = ModsCacheDir::initialize(path).unwrap();
                let _reader = &cache;
            });
        }
    });
    let cache = ModsCacheDir::new(root.path().join("cache")).unwrap();
    assert!(cache.list_metadata().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn default_cache_process_worker() {
    let Some(expected) = std::env::var_os("KAKO_DEFAULT_CACHE_EXPECT") else {
        return;
    };
    let result = kako_mods_lib::local::cache::mods::ModsCacheSelection::Default.open();
    if expected == "valid" {
        assert!(result.is_ok(), "{result:?}");
    } else {
        assert!(result.is_err(), "{result:?}");
    }
}

#[cfg(unix)]
#[test]
fn default_directory_initialization_is_coordinated_across_processes_and_temp_directories() {
    use std::process::{Command, Stdio};
    let home = tempfile::tempdir().unwrap();
    let cache_path = home.path().join(".mcpm/cache/mods");
    for state in ["absent", "empty", "foreign", "legacy-lock"] {
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).unwrap();
        }
        if state != "absent" {
            fs::create_dir_all(&cache_path).unwrap();
        }
        if state == "foreign" {
            fs::write(cache_path.join("data"), b"keep").unwrap();
        }
        if state == "legacy-lock" {
            fs::write(cache_path.join(".lock"), b"").unwrap();
        }
        let valid = matches!(state, "absent" | "empty");
        let mut children = Vec::new();
        for index in 0..4 {
            let temp = home.path().join(format!("temp-{index}"));
            fs::create_dir_all(&temp).unwrap();
            children.push(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "default_cache_process_worker"])
                    .env("HOME", home.path())
                    .env("USERPROFILE", home.path())
                    .env("TMPDIR", &temp)
                    .env("TEMP", &temp)
                    .env("TMP", &temp)
                    .env(
                        "KAKO_DEFAULT_CACHE_EXPECT",
                        if valid { "valid" } else { "broken" },
                    )
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
        }
        for mut child in children {
            assert!(child.wait().unwrap().success(), "state: {state}");
        }
        assert_eq!(cache_path.join(".mcpm/metadata.json").exists(), valid);
        if state == "foreign" {
            assert!(!cache_path.join(".lock").exists());
        }
    }
    #[cfg(unix)]
    assert!(ModsCacheDir::initialize("/").is_err());
}

#[test]
fn unusable_cached_content_falls_back_to_loader_and_source_identification() {
    for kind in ["directory", "corrupt", "symlink"] {
        #[cfg(not(unix))]
        if kind == "symlink" {
            continue;
        }
        let root = tempfile::tempdir().unwrap();
        let (cache, source) = fixture(root.path());
        let (loaders, fabric_calls, forge_calls) = loaders();
        let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
        cache
            .refresh_metadata(key, &loaders, MetadataRefresh::Force)
            .unwrap();
        let stored = cache.content_path(&key);
        fs::remove_file(&stored).unwrap();
        match kind {
            "directory" => fs::create_dir(&stored).unwrap(),
            "corrupt" => fs::write(
                &stored,
                vec![0; fs::metadata(&source).unwrap().len() as usize],
            )
            .unwrap(),
            _ => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&source, &stored).unwrap();
            }
        }
        fabric_calls.store(0, Ordering::SeqCst);
        forge_calls.store(0, Ordering::SeqCst);
        let report = open(&source, &loaders, &context(&cache));
        assert_eq!(report.file.metadata[0].mods[0].modid, "example");
        assert_eq!(report.cache.reuse(), mcmod::ModCacheReuse::None);
        assert_eq!(fabric_calls.load(Ordering::SeqCst), 1);
        assert_eq!(forge_calls.load(Ordering::SeqCst), 1);
        let source_snapshot = cache.find_ident_info(&source).unwrap().unwrap();
        assert!(matches!(
            source_snapshot.status(),
            kako_mods_lib::local::cache::mods::CacheStatus::Unavailable(_)
        ));
        assert_eq!(source_snapshot.matches().count(), 0);
        // Errors in the actual input must remain errors rather than cache misses.
        assert!(
            cache
                .find_metadata(&root.path().join("missing.jar"))
                .is_err()
        );
    }
}
