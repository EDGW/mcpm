//! Lookup integration: real sampling collisions, read-only fallback and pinned handles.

use kako_mods_lib::local::cache::{
    CacheContent, CacheDir, CacheLinkPreference, ContentVerification,
    codec::CacheDirMetadata,
    hash::{self, Blake3Hash},
};
use rusqlite::{Connection, params};
use std::{
    fs,
    io::{Cursor, Read, Seek, SeekFrom},
};

fn read(content: &CacheContent) -> Vec<u8> {
    let mut bytes = Vec::new();
    content.file().read_to_end(&mut bytes).unwrap();
    content.validate().unwrap();
    bytes
}

fn index_path(cache: &CacheDir) -> std::path::PathBuf {
    cache.path().join(".mcpm/index.sqlite3")
}

fn erase_index(cache: &CacheDir) {
    for name in [
        "index.sqlite3",
        "index.sqlite3-wal",
        "index.sqlite3-shm",
        "index.lock",
    ] {
        let path = cache.path().join(".mcpm").join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn sampling_collisions_never_match_independent_content_and_hardlinks_use_identity() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let first = vec![b'a'; 256 * 1024];
    let mut collision = first.clone();
    collision[32 * 1024] = b'b';
    assert_eq!(
        hash::imohash(&first).unwrap(),
        hash::imohash(&collision).unwrap()
    );
    let source = temp.path().join("source");
    let other = temp.path().join("collision");
    let copy = temp.path().join("independent-copy");
    fs::write(&source, &first).unwrap();
    fs::write(&other, &collision).unwrap();
    fs::write(&copy, &first).unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    cache
        .write_metadata(&key, "test", &CacheDirMetadata::new())
        .unwrap()
        .unwrap();
    assert_eq!(cache.find_file(&other).unwrap(), None);
    let matched = cache.find_content(&copy).unwrap().unwrap();
    assert_eq!(matched.key(), key);
    assert_eq!(matched.verification(), ContentVerification::Blake3);
    assert_eq!(read(&matched), first);

    let linked = temp.path().join("linked");
    cache
        .export(&key, &[linked.clone()], CacheLinkPreference::HardLink)
        .unwrap()
        .unwrap();
    let matched = cache.find_content(&linked).unwrap().unwrap();
    assert_eq!(matched.key(), key);
    assert_eq!(matched.verification(), ContentVerification::FileIdentity);
    assert_eq!(read(&matched), first);
    assert_eq!(
        matched
            .try_read_metadata::<CacheDirMetadata>("test")
            .unwrap(),
        Some(CacheDirMetadata::new())
    );
    let second = cache.import(&other, CacheLinkPreference::Copy).unwrap();
    assert_ne!(second, key);
    assert_eq!(cache.find_file(&other).unwrap(), Some(second));
    assert_eq!(cache.find_file(&copy).unwrap(), Some(key));
    assert_eq!(cache.list().unwrap().len(), 2);
}

#[test]
fn missing_corrupt_and_incompatible_indexes_fall_back_without_repair() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let source = temp.path().join("source");
    fs::write(&source, vec![3; 256 * 1024]).unwrap();
    let key = cache
        .import(&source, CacheLinkPreference::HardLink)
        .unwrap();
    erase_index(&cache);
    let content = cache.find_content(&source).unwrap().unwrap();
    assert_eq!(content.key(), key);
    assert_eq!(content.verification(), ContentVerification::Blake3);
    assert!(!index_path(&cache).exists());
    assert!(!cache.path().join(".mcpm/index.lock").exists());
    assert_eq!(cache.rebuild_index().unwrap(), 1);
    assert_eq!(
        cache.find_content(&source).unwrap().unwrap().verification(),
        ContentVerification::FileIdentity
    );

    erase_index(&cache);
    fs::write(index_path(&cache), b"not a SQLite database").unwrap();
    assert_eq!(
        cache.find_content(&source).unwrap().unwrap().verification(),
        ContentVerification::Blake3
    );
    assert_eq!(
        fs::read(index_path(&cache)).unwrap(),
        b"not a SQLite database"
    );
    let second = temp.path().join("second");
    fs::write(&second, b"still imports despite damaged index").unwrap();
    cache.import(&second, CacheLinkPreference::Copy).unwrap();
    assert_eq!(
        fs::read(index_path(&cache)).unwrap(),
        b"not a SQLite database"
    );
    assert_eq!(cache.rebuild_index().unwrap(), 2);

    let connection = Connection::open(index_path(&cache)).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    drop(connection);
    assert_eq!(
        cache.find_content(&source).unwrap().unwrap().verification(),
        ContentVerification::Blake3
    );
    let connection = Connection::open(index_path(&cache)).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        999
    );
    drop(connection);
    assert_eq!(cache.rebuild_index().unwrap(), 2);
}

#[test]
fn stale_candidates_are_read_only_misses_and_rebuild_skips_corrupt_objects() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let source = temp.path().join("source");
    let bytes = vec![8; 256 * 1024];
    fs::write(&source, &bytes).unwrap();
    let key = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let quick = hash::imohash(&bytes).unwrap();
    let missing = Blake3Hash::new([1; 32]);
    let connection = Connection::open(index_path(&cache)).unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO quick_lookup(imohash, blake3) VALUES (?1, ?2)",
            params![quick.as_bytes().as_slice(), missing.as_bytes().as_slice()],
        )
        .unwrap();
    drop(connection);
    assert_eq!(cache.find_file(&source).unwrap(), Some(key));
    let stored = cache.open_content(&key).unwrap().unwrap().path().to_owned();
    fs::remove_file(&stored).unwrap();
    assert_eq!(cache.find_file(&source).unwrap(), None);
    let connection = Connection::open(index_path(&cache)).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM quick_lookup", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(connection);
    // Same-sized wrong content must not be blessed by rebuilding the index.
    fs::write(&stored, vec![9; bytes.len()]).unwrap();
    assert_eq!(cache.rebuild_index().unwrap(), 0);
    assert!(cache.check(&key).is_err());
    assert!(cache.find_file(&source).is_err());
}

#[test]
fn streaming_imohash_matches_library_at_sampling_boundaries() {
    for size in [0, 5, 128 * 1024 - 1, 128 * 1024, 256 * 1024, 512 * 1024] {
        let bytes: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut reader = Cursor::new(&bytes);
        reader.seek(SeekFrom::End(0)).unwrap();
        let sampled = hash::imohash_reader(&mut reader).unwrap();
        assert_eq!(
            *sampled.as_bytes(),
            imohash::Hasher::new().sum(&bytes).unwrap().to_le_bytes()
        );
        assert_eq!(sampled, hash::imohash(&bytes).unwrap());
    }
}

#[cfg(unix)]
#[test]
fn lookup_handles_whitespace_and_platform_paths_without_reopening_by_string() {
    let temp = tempfile::tempdir().unwrap();
    let cache = CacheDir::initialize(temp.path().join("cache")).unwrap();
    let source = temp.path().join(" name with spaces ");
    let bytes = vec![4; 128 * 1024];
    fs::write(&source, &bytes).unwrap();
    let key = cache
        .import(&source, CacheLinkPreference::HardLink)
        .unwrap();
    let content = cache.find_content(&source).unwrap().unwrap();
    assert_eq!(content.key(), key);
    assert_eq!(content.verification(), ContentVerification::FileIdentity);
    assert_eq!(read(&content), bytes);
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let non_utf8 = temp.path().join(OsStr::from_bytes(b"name-\xff"));
        fs::write(&non_utf8, &bytes).unwrap();
        let content = cache.find_content(&non_utf8).unwrap().unwrap();
        assert_eq!(content.key(), key);
        assert_eq!(read(&content), bytes);
    }
}
