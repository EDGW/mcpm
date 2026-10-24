//! Cache management checks for initialization, diagnostic inventory and owned-name deletion.

use kako_mods_lib::local::cache::{
    CacheDir, CacheFileKind, CacheLinkPreference, TransferMethod, codec::CacheDirMetadata,
    hash::Blake3Hash,
};
use std::{
    fs,
    io::Read,
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn initialization_is_concurrent_idempotent_and_never_adopts_nonempty_or_old_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("new");
    assert!(CacheDir::new(root.clone()).is_err());
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                CacheDir::initialize(root).unwrap()
            })
        })
        .collect();
    let caches: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    for cache in &caches {
        assert_eq!(cache.path(), caches[0].path());
    }
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.join(".mcpm")).unwrap().count(), 1);
    assert!(CacheDir::initialize(&root).is_ok());

    let occupied = temp.path().join("occupied");
    fs::create_dir(&occupied).unwrap();
    fs::write(occupied.join("sentinel"), b"untouched").unwrap();
    assert!(CacheDir::initialize(&occupied).is_err());
    assert!(!occupied.join(".mcpm").exists());
    assert_eq!(fs::read(occupied.join("sentinel")).unwrap(), b"untouched");
    let old = temp.path().join("old");
    fs::create_dir_all(old.join(".mcpm")).unwrap();
    fs::write(old.join(".mcpm/mcpm.json"), br#"{"version":2}"#).unwrap();
    assert!(CacheDir::initialize(&old).is_err());
    assert!(!old.join(".mcpm/metadata.json").exists());
    let manifest = caches[0].metadata_path();
    fs::write(&manifest, br#"{"version":999}"#).unwrap();
    assert!(CacheDir::initialize(&root).is_err());
    assert_eq!(fs::read(&manifest).unwrap(), br#"{"version":999}"#);
}

#[test]
fn diagnostics_find_orphans_and_invalid_types_while_removal_preserves_external_links() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let source = temp.path().join("source");
    fs::write(&source, b"protected shared content").unwrap();
    let key = cache
        .import(&source, CacheLinkPreference::HardLink)
        .unwrap();
    cache
        .write_metadata(&key, "test", &CacheDirMetadata::new())
        .unwrap()
        .unwrap();
    let content = cache.open_content(&key).unwrap().unwrap();
    let stored = content.path().to_owned();
    drop(content);
    let unrelated = stored.with_extension("test");
    let temporary = stored.with_extension("test.json.tmp");
    fs::write(&unrelated, b"unowned name").unwrap();
    fs::write(&temporary, b"unpublished metadata").unwrap();
    fs::remove_file(&stored).unwrap();
    assert!(cache.list().unwrap().is_empty());
    let inspection = cache.inspect(&key).unwrap();
    assert!(inspection.content.is_none());
    assert_eq!(inspection.metadata[0].name, "test");
    let scanned = cache.scan().unwrap();
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].key, key);
    assert!(cache.remove(&key).unwrap());
    assert!(!cache.remove(&key).unwrap());
    assert!(cache.scan().unwrap().is_empty());
    assert_eq!(fs::read(&unrelated).unwrap(), b"unowned name");
    assert_eq!(fs::read(&temporary).unwrap(), b"unpublished metadata");
    assert_eq!(fs::read(&source).unwrap(), b"protected shared content");

    let bad_key = Blake3Hash::new([0; 32]);
    let name = bad_key.to_string();
    let invalid = cache.path().join(&name[..2]).join(&name);
    fs::create_dir_all(&invalid).unwrap();
    fs::write(invalid.join("sentinel"), b"never recursively delete").unwrap();
    assert_eq!(
        cache.inspect(&bad_key).unwrap().content,
        Some(CacheFileKind::Directory)
    );
    assert!(cache.entry(&bad_key).unwrap().is_none());
    assert!(cache.remove(&bad_key).is_err());
    assert_eq!(
        fs::read(invalid.join("sentinel")).unwrap(),
        b"never recursively delete"
    );
}

#[test]
fn outcomes_report_publication_and_real_method_and_handles_detect_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let source = temp.path().join("source");
    fs::write(&source, b"read from offset zero").unwrap();
    let first = cache
        .import_with_outcome(&source, CacheLinkPreference::Copy)
        .unwrap();
    assert!(first.published);
    assert_eq!(first.method, Some(TransferMethod::Copy));
    let reused = cache
        .import_with_outcome(&source, CacheLinkPreference::HardLink)
        .unwrap();
    assert_eq!(reused.key, first.key);
    assert!(!reused.published);
    assert_eq!(reused.method, None);
    let linked = temp.path().join("linked");
    let outcomes = cache
        .export_with_outcomes(&first.key, &[linked.clone()], CacheLinkPreference::HardLink)
        .unwrap()
        .unwrap();
    assert_eq!(outcomes[0].method, TransferMethod::HardLink);
    assert_eq!(outcomes[0].destination, linked.canonicalize().unwrap());
    let copied = temp.path().join("copied");
    let outcomes = cache
        .export_with_outcomes(&first.key, &[copied.clone()], CacheLinkPreference::Copy)
        .unwrap()
        .unwrap();
    assert_eq!(outcomes[0].method, TransferMethod::Copy);
    fs::write(&copied, b"independent bytes").unwrap();
    let handle = cache.open_content(&first.key).unwrap().unwrap();
    let mut bytes = Vec::new();
    handle.file().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"read from offset zero");
    fs::remove_file(handle.path()).unwrap();
    fs::write(handle.path(), b"replaced object").unwrap();
    assert!(handle.validate().is_err());
    assert!(cache.check(&first.key).is_err());
}
