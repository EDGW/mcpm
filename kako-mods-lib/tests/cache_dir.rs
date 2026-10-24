//! Generic cache integration checks for publication, link identity and failure isolation.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use kako_mods_lib::local::{
    cache::{CacheDir, CacheLinkPreference, codec::CacheDirMetadata, hash::Blake3Hash},
    files::FileIdentity,
};
use tempfile::TempDir;

fn cache(root: &Path) -> CacheDir {
    CacheDir::initialize(root).unwrap()
}

fn object(cache: &CacheDir, key: &Blake3Hash) -> PathBuf {
    let name = blake3::Hash::from_bytes(*key.as_bytes()).to_hex();
    cache.path().join(&name[..2]).join(name.as_str())
}

#[test]
fn duplicate_import_preserves_existing_inode_and_duplicate_source() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    let duplicate = temp.path().join("duplicate");
    fs::write(&source, b"identical bytes").unwrap();
    fs::write(&duplicate, b"identical bytes").unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let stored = object(&cache, &key);
    let original_identity = FileIdentity::read(&stored).unwrap();
    assert_ne!(original_identity, FileIdentity::read(&source).unwrap());

    let duplicate_key = cache
        .import(&duplicate, CacheLinkPreference::HardLink)
        .unwrap();
    assert_eq!(key.as_bytes(), duplicate_key.as_bytes());
    assert_eq!(original_identity, FileIdentity::read(&stored).unwrap());
    assert_ne!(original_identity, FileIdentity::read(&duplicate).unwrap());
    // Reusing existing content must not protect or relink the duplicate source.
    fs::write(&duplicate, b"independent replacement bytes").unwrap();
    assert_eq!(fs::read(stored).unwrap(), b"identical bytes");
}

#[test]
fn export_preserves_link_semantics_and_preflights_all_destinations() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    fs::write(&source, b"original bytes").unwrap();
    let key = cache
        .import(&source, CacheLinkPreference::HardLink)
        .unwrap();
    let stored = object(&cache, &key);
    let identity = FileIdentity::read(&stored).unwrap();
    assert_eq!(identity, FileIdentity::read(&source).unwrap());
    let linked = temp.path().join("linked");
    cache
        .export(&key, &[linked.clone()], CacheLinkPreference::HardLink)
        .unwrap()
        .unwrap();
    assert_eq!(identity, FileIdentity::read(&linked).unwrap());

    let copied = temp.path().join("copied");
    cache
        .export(&key, &[copied.clone()], CacheLinkPreference::Copy)
        .unwrap()
        .unwrap();
    assert_ne!(identity, FileIdentity::read(&copied).unwrap());
    fs::write(&copied, b"changed independently").unwrap();
    assert_eq!(fs::read(&stored).unwrap(), b"original bytes");
    let untouched = temp.path().join("untouched");
    assert!(
        cache
            .export(
                &key,
                &[untouched.clone(), copied.clone()],
                CacheLinkPreference::Copy
            )
            .is_err()
    );
    assert!(!untouched.exists());
    assert_eq!(fs::read(&copied).unwrap(), b"changed independently");
    assert!(
        cache
            .export(&key, &[cache.path().join("bad")], CacheLinkPreference::Copy)
            .is_err()
    );
    assert!(!cache.path().join("bad").exists());
}

#[test]
fn failed_metadata_encoding_preserves_snapshot_and_orphans_are_ignored() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    fs::write(&source, b"content with metadata").unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let current = CacheDirMetadata::new();
    cache
        .write_metadata(&key, "test", &current)
        .unwrap()
        .unwrap();
    assert!(
        cache
            .write_metadata(&key, "test", &CacheDirMetadata { version: 2 })
            .is_err()
    );
    assert_eq!(
        cache
            .try_read_metadata::<CacheDirMetadata>(&key, "test")
            .unwrap(),
        Some(current)
    );
    let stored = object(&cache, &key);
    let sidecar = stored.with_extension("test.json");
    fs::write(&sidecar, b"invalid JSON").unwrap();
    assert!(
        cache
            .try_read_metadata::<CacheDirMetadata>(&key, "test")
            .is_err()
    );
    fs::remove_file(&stored).unwrap();
    assert!(
        cache
            .try_read_metadata::<CacheDirMetadata>(&key, "test")
            .unwrap()
            .is_none()
    );
    assert!(
        cache
            .write_metadata(&key, "test", &current)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read(&sidecar).unwrap(), b"invalid JSON");
}

#[test]
fn corrupt_existing_content_is_never_reused_exported_or_blessed_by_metadata() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    fs::write(&source, b"good").unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let stored = object(&cache, &key);
    // Replace the name instead of writing through the protected cache inode.
    fs::remove_file(&stored).unwrap();
    fs::write(&stored, b"evil").unwrap();
    assert!(cache.check(&key).is_err());
    // Inventory still exposes corrupt objects so callers can explicitly check them.
    assert!(cache.entry(&key).unwrap().is_some());
    assert!(
        cache
            .list()
            .unwrap()
            .iter()
            .any(|listed| listed.as_bytes() == key.as_bytes())
    );
    assert!(cache.import(&source, CacheLinkPreference::Copy).is_err());
    let destination = temp.path().join("export");
    assert!(
        cache
            .export(&key, &[destination.clone()], CacheLinkPreference::Copy)
            .is_err()
    );
    assert!(!destination.exists());
    assert!(
        cache
            .write_metadata(&key, "test", &CacheDirMetadata::new())
            .is_err()
    );
    assert!(
        cache
            .try_read_metadata::<CacheDirMetadata>(&key, "test")
            .is_err()
    );
    assert_eq!(fs::read(&stored).unwrap(), b"evil");
}

#[test]
fn missing_content_and_invalid_metadata_names_create_no_objects() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let key = Blake3Hash::new([0; 32]);
    assert!(cache.check(&key).unwrap().is_none());
    assert!(cache.entry(&key).unwrap().is_none());
    assert!(cache.list().unwrap().is_empty());
    let destination = temp.path().join("export");
    assert!(
        cache
            .export(
                &key,
                &[destination.clone()],
                CacheLinkPreference::PreferHardLink
            )
            .unwrap()
            .is_none()
    );
    assert!(
        cache
            .write_metadata(&key, "test", &CacheDirMetadata::new())
            .unwrap()
            .is_none()
    );
    for name in [
        "",
        "../outside",
        "x/y",
        "x\\y",
        "name:stream",
        "name\0",
        "name\n",
        "name*",
        "name?",
        "name\"",
        "name<",
        "name>",
        "name|",
    ] {
        assert!(
            cache
                .write_metadata(&key, name, &CacheDirMetadata::new())
                .is_err()
        );
    }
    assert!(!destination.exists());
    assert!(!object(&cache, &key).parent().unwrap().exists());
}

#[test]
fn concurrent_publishers_converge_on_one_verified_inode() {
    let temp = TempDir::new().unwrap();
    let cache = Arc::new(cache(&temp.path().join("cache")));
    let source = temp.path().join("source");
    fs::write(&source, vec![7; 256 * 1024]).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let source = source.clone();
            thread::spawn(move || {
                barrier.wait();
                let key = cache
                    .import(&source, CacheLinkPreference::PreferHardLink)
                    .unwrap();
                (FileIdentity::read(&object(&cache, &key)).unwrap(), key)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    let (identity, key) = &results[0];
    for (observed_identity, observed_key) in &results {
        assert_eq!(observed_identity, identity);
        assert_eq!(observed_key.as_bytes(), key.as_bytes());
    }
    let stored = object(&cache, key);
    assert_eq!(fs::read(&stored).unwrap(), vec![7; 256 * 1024]);
    assert_eq!(fs::read_dir(stored.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn inventory_groups_metadata_and_ignores_orphans_noise_and_misplaced_files() {
    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    fs::write(&source, b"first object").unwrap();
    let first = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    fs::write(&source, b"second object").unwrap();
    let second = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    for name in [
        "z",
        "a.detail",
        "json",
        "loader details",
        "trailing.",
        "元数据",
    ] {
        cache
            .write_metadata(&first, name, &CacheDirMetadata::new())
            .unwrap()
            .unwrap();
        assert_eq!(
            cache
                .try_read_metadata::<CacheDirMetadata>(&first, name)
                .unwrap(),
            Some(CacheDirMetadata::new())
        );
    }
    let first_path = object(&cache, &first);
    fs::create_dir(first_path.with_extension("directory.json")).unwrap();
    fs::write(first_path.with_extension("json"), b"missing name").unwrap();
    fs::write(first_path.with_extension("legacy"), b"missing suffix").unwrap();
    fs::write(first_path.with_extension("partial.json.tmp"), b"temporary").unwrap();
    fs::write(first_path.with_extension("invalid."), b"noise").unwrap();
    fs::write(first_path.parent().unwrap().join("temporary"), b"noise").unwrap();
    fs::write(cache.path().join("README"), b"noise").unwrap();

    let orphan = Blake3Hash::new([0; 32]);
    let orphan_path = object(&cache, &orphan);
    fs::create_dir_all(orphan_path.parent().unwrap()).unwrap();
    fs::write(orphan_path.with_extension("orphan.json"), b"orphan").unwrap();
    // A valid hash name in the wrong shard must not become a second object.
    let misplaced = Blake3Hash::new(*blake3::hash(b"misplaced").as_bytes());
    let misplaced_name = blake3::Hash::from_bytes(*misplaced.as_bytes()).to_hex();
    let shard = if &misplaced_name[..2] == "00" {
        "ff"
    } else {
        "00"
    };
    fs::create_dir_all(cache.path().join(shard)).unwrap();
    fs::write(
        cache.path().join(shard).join(misplaced_name.as_str()),
        b"misplaced",
    )
    .unwrap();

    let listed: Vec<_> = cache
        .list()
        .unwrap()
        .iter()
        .map(|key| *key.as_bytes())
        .collect();
    let mut expected = vec![*first.as_bytes(), *second.as_bytes()];
    expected.sort_unstable();
    assert_eq!(listed, expected);
    let entry = cache.entry(&first).unwrap().unwrap();
    assert_eq!(entry.blake3.as_bytes(), first.as_bytes());
    assert_eq!(
        entry.metanames,
        [
            "a.detail",
            "json",
            "loader details",
            "trailing.",
            "z",
            "元数据"
        ]
    );
    assert!(cache.entry(&second).unwrap().unwrap().metanames.is_empty());
    assert!(cache.entry(&orphan).unwrap().is_none());
    assert!(cache.entry(&misplaced).unwrap().is_none());
    assert!(cache.check(&first).unwrap().is_some());
    assert!(cache.check(&second).unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn inventory_ignores_linked_names_and_metadata() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let cache = cache(&temp.path().join("cache"));
    let source = temp.path().join("source");
    fs::write(&source, b"real object").unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let stored = object(&cache, &key);
    symlink(&source, stored.with_extension("linked.json")).unwrap();
    assert!(cache.entry(&key).unwrap().unwrap().metanames.is_empty());

    let linked_key = Blake3Hash::new(*blake3::hash(b"linked object").as_bytes());
    let linked_path = object(&cache, &linked_key);
    fs::create_dir_all(linked_path.parent().unwrap()).unwrap();
    symlink(&source, &linked_path).unwrap();
    let listed = cache.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_bytes(), key.as_bytes());
    assert!(cache.check(&linked_key).is_err());
    assert!(cache.entry(&linked_key).unwrap().is_none());

    let shard_byte = (0u8..=255)
        .find(|byte| !cache.path().join(format!("{byte:02x}")).exists())
        .unwrap();
    let outside_key = Blake3Hash::new([shard_byte; 32]);
    let outside_name = blake3::Hash::from_bytes(*outside_key.as_bytes()).to_hex();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join(outside_name.as_str()), b"outside object").unwrap();
    symlink(&outside, object(&cache, &outside_key).parent().unwrap()).unwrap();
    assert!(cache.entry(&outside_key).unwrap().is_none());
    let listed = cache.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_bytes(), key.as_bytes());
}
