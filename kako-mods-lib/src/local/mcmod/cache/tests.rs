//! Internal metadata commits: provenance checks, loader merging and atomic rejection.

use std::{collections::HashMap, fs, io::Cursor, io::Write};

use sha2::{Digest, Sha256};

use super::{
    CacheEntry, CacheError, CacheId, CacheStatus, CacheWriter, CachedIdentification, ModsCache,
    storage,
};
use crate::local::{
    mcmod::{ModEntry, ModMeta},
    modloader::{BUILTIN_LOADERS, LoaderId},
};

#[test]
fn explicit_metadata_preserves_nested_recognition_and_unselected_loader_results() {
    let root = tempfile::tempdir().unwrap();
    let bytes = fabric_archive();
    let source = root.path().join("mod.jar");
    fs::write(&source, &bytes).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let id = writer.import(&source).unwrap();
    let empty = write_identified(&mut writer, id, &bytes, HashMap::new()).unwrap();
    assert!(!empty.digest_path().exists());

    let fabric = BUILTIN_LOADERS.get("fabric").unwrap();
    let metadata = fabric
        .instance()
        .try_identify(&ModEntry::new(source).unwrap())
        .unwrap();
    write_identified(
        &mut writer,
        id,
        &bytes,
        [(fabric.id().clone(), metadata)].into_iter().collect(),
    )
    .unwrap();
    let entry = write_identified(
        &mut writer,
        id,
        &bytes,
        [("forge".into(), None)].into_iter().collect(),
    )
    .unwrap();
    assert_eq!(entry.status(), &CacheStatus::Available);
    let digest = entry.digest().unwrap();
    assert!(matches!(
        digest.identification("forge"),
        CachedIdentification::Unrecognized
    ));
    assert!(matches!(
        digest.identification("neoforge"),
        CachedIdentification::Missing
    ));
    let CachedIdentification::Recognized(cached) = digest.identification("fabric") else {
        panic!("expected Fabric metadata");
    };
    let restored = cached.to_mod_meta(&BUILTIN_LOADERS).unwrap();
    assert_eq!(restored.mods[0].modid, "outer");
    assert_eq!(restored.nested[0].meta.mods[0].modid, "child");
    assert_eq!(restored.nested[0].path, "META-INF/jars/child.jar");

    // Use the codec owner to simulate an older parser version, not a second JSON parser.
    let mut outdated = storage::read_digest(entry.digest_path()).unwrap();
    outdated.metadata_version = 0;
    storage::write_digest(entry.digest_path(), &outdated, true).unwrap();
    let entry = write_identified(
        &mut writer,
        id,
        &bytes,
        [("forge".into(), None)].into_iter().collect(),
    )
    .unwrap();
    assert_eq!(entry.status(), &CacheStatus::Available);
    assert!(matches!(
        entry.digest().unwrap().identification("fabric"),
        CachedIdentification::Missing
    ));
}

#[test]
fn explicit_metadata_rejects_wrong_provenance_and_invalid_batches_atomically() {
    let root = tempfile::tempdir().unwrap();
    let bytes = fabric_archive();
    let source = root.path().join("mod.jar");
    fs::write(&source, &bytes).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let id = writer.import(&source).unwrap();
    let unrecognized = || [("forge".into(), None)].into_iter().collect();

    assert!(write_identified(&mut writer, id, b"wrong bytes", unrecognized()).is_err());
    assert!(!writer.get(id).digest_path().exists());
    let entry = write_identified(&mut writer, id, &bytes, unrecognized()).unwrap();
    let original = fs::read(entry.digest_path()).unwrap();
    let fabric = BUILTIN_LOADERS.get("fabric").unwrap();
    let identify = || {
        fabric
            .instance()
            .try_identify(&ModEntry::new(source.clone()).unwrap())
            .unwrap()
            .unwrap()
    };
    // This deliberately violates the internal contract to exercise rejection.
    assert!(
        write_identified(
            &mut writer,
            id,
            &bytes,
            [("forge".into(), Some(identify()))].into_iter().collect(),
        )
        .is_err()
    );
    assert_eq!(fs::read(entry.digest_path()).unwrap(), original);

    let mut invalid = identify();
    invalid.nested[0].path = "../outside.jar".to_owned();
    assert!(
        write_identified(
            &mut writer,
            id,
            &bytes,
            [("fabric".into(), Some(invalid)), ("neoforge".into(), None)]
                .into_iter()
                .collect(),
        )
        .is_err()
    );
    assert_eq!(fs::read(entry.digest_path()).unwrap(), original);

    fs::remove_file(entry.data_path()).unwrap();
    fs::write(entry.data_path(), vec![b'x'; bytes.len()]).unwrap();
    assert!(write_identified(&mut writer, id, &bytes, unrecognized()).is_err());
    assert_eq!(fs::read(entry.digest_path()).unwrap(), original);
    writer.remove(id).unwrap();
    assert!(matches!(
        write_identified(&mut writer, id, &bytes, unrecognized()),
        Err(CacheError::NotFound(_))
    ));
    assert!(!entry.digest_path().exists());
}

fn write_identified(
    writer: &mut CacheWriter<'_>,
    id: CacheId,
    bytes: &[u8],
    metadata: HashMap<LoaderId, Option<ModMeta>>,
) -> Result<CacheEntry, CacheError> {
    // Never expose or use write_metadata outside this crate under any circumstances:
    // incorrect associations destroy data consistency. Tests supply the identified
    // bytes explicitly; negative cases deliberately violate this contract.
    writer
        .write_metadata(id, &format!("{:x}", Sha256::digest(bytes)), &metadata)
        .map(|(entry, _)| entry)
}

fn fabric_archive() -> Vec<u8> {
    let child = archive(&[(
        "fabric.mod.json",
        br#"{"schemaVersion":1,"id":"child","version":"2"}"#.to_vec(),
    )]);
    archive(&[
        (
            "fabric.mod.json",
            br#"{"schemaVersion":1,"id":"outer","version":"1","jars":[{"file":"META-INF/jars/child.jar"}]}"#.to_vec(),
        ),
        ("META-INF/jars/child.jar", child),
    ])
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

#[test]
fn quick_hash_collisions_stale_candidates_and_corrupt_index_fall_back_safely() {
    use super::{CacheLookup, ExportMode, content::ContentFile, index};

    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.jar");
    let second = root.path().join("second.jar");
    let a = vec![b'a'; 256 * 1024];
    let mut b = a.clone();
    b[32 * 1024] = b'b'; // Outside all imohash sampling regions.
    fs::write(&first, &a).unwrap();
    fs::write(&second, &b).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let mut writer = cache.writer().unwrap();
    let a_id = writer.import(&first).unwrap();
    let b_id = writer.import(&second).unwrap();
    assert_ne!(a_id, b_id);
    let quick = ContentFile::open(&first).unwrap().quick_hash().unwrap();
    assert_eq!(
        quick,
        ContentFile::open(&second).unwrap().quick_hash().unwrap()
    );
    let candidates = index::candidates(cache.root(), &quick);
    assert!(candidates.contains(&a_id) && candidates.contains(&b_id));

    // Independent copies share the quick bucket but must use full-content identity.
    let lookup = CacheLookup::new(ModsCache::new(cache.root())).unwrap();
    assert_eq!(lookup.find(&first).unwrap().unwrap().id(), a_id);
    assert_eq!(lookup.find(&second).unwrap().unwrap().id(), b_id);
    let linked = root.path().join("linked.jar");
    writer.export(a_id, &linked, ExportMode::HardLink).unwrap();
    assert_eq!(lookup.find(&linked).unwrap().unwrap().id(), a_id);

    writer.remove(a_id).unwrap();
    index::insert(cache.root(), &quick, a_id).unwrap(); // Deliberately stale.
    assert!(lookup.find(&first).unwrap().is_none());
    assert_eq!(lookup.find(&second).unwrap().unwrap().id(), b_id);

    // Index corruption cannot turn an index miss into a content miss or cause writes
    // from a query. Only the explicit rebuild repairs the derived database.
    let database = cache.root().join(".mcpm/index.sqlite3");
    fs::write(&database, b"broken SQLite").unwrap();
    let broken = fs::read(&database).unwrap();
    assert_eq!(lookup.find(&second).unwrap().unwrap().id(), b_id);
    assert_eq!(fs::read(&database).unwrap(), broken);
    assert_eq!(writer.rebuild_index().unwrap(), 1);
    assert_eq!(index::candidates(cache.root(), &quick), vec![b_id]);

    fs::remove_file(&database).unwrap();
    assert_eq!(lookup.find(&second).unwrap().unwrap().id(), b_id);
    assert!(!database.exists());
    assert_eq!(writer.rebuild_index().unwrap(), 1);
}

#[test]
fn concurrent_metadata_publications_are_complete_valid_snapshots() {
    use super::{MOD_METADATA_VERSION, MetadataRefresh};
    use std::sync::{Arc, Barrier};

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("empty.jar");
    fs::write(&source, archive(&[])).unwrap();
    let cache = ModsCache::new(root.path().join("cache"));
    cache.initialize().unwrap();
    let id = cache.writer().unwrap().import(&source).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let path = cache.root().to_owned();
    std::thread::scope(|scope| {
        for loader in ["fabric", "forge", "neoforge"] {
            let barrier = barrier.clone();
            let path = path.clone();
            scope.spawn(move || {
                let cache = ModsCache::new(path);
                let mut writer = cache.writer().unwrap();
                let selected = [(loader.into(), BUILTIN_LOADERS.get(loader).unwrap().clone())]
                    .into_iter()
                    .collect();
                barrier.wait();
                for _ in 0..25 {
                    writer
                        .refresh_metadata(id, &selected, MetadataRefresh::Force)
                        .unwrap();
                }
            });
        }
        barrier.wait();
        let reader = cache.reader().unwrap();
        for _ in 0..100 {
            let entry = reader.get(id);
            match entry.status() {
                CacheStatus::Unavailable(super::CacheUnavailableReason::MissingDigest) => {}
                CacheStatus::Available => {
                    let digest = entry.digest().unwrap();
                    assert_eq!(digest.metadata_version(), MOD_METADATA_VERSION);
                    assert!(digest.valid_metadata());
                    assert_eq!(digest.blake3, id.to_string());
                    assert!(
                        digest.identifications().all(|(_, result)| matches!(
                            result,
                            CachedIdentification::Unrecognized
                        ))
                    );
                }
                status => panic!("observed torn/invalid metadata: {status:?}"),
            }
        }
    });
    let entry = cache.reader().unwrap().get(id);
    assert_eq!(entry.status(), &CacheStatus::Available);
    // We deliberately do not require the union of all three writers' results.
}

#[test]
fn legacy_guid_directory_is_rejected_without_modification() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    let manifest = root.path().join(".mcpm/mcpm.json");
    let old = b"{\"version\":1}\n";
    fs::write(&manifest, old).unwrap();
    let cache = ModsCache::new(root.path());
    assert!(matches!(
        cache.reader(),
        Err(CacheError::UnsupportedDirectoryVersion { version: 1, .. })
    ));
    assert!(cache.initialize().is_err());
    assert_eq!(fs::read(manifest).unwrap(), old);
}
