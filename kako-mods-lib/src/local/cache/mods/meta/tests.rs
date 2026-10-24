//! Internal metadata commits: provenance checks, loader merging and atomic rejection.

use std::{collections::HashMap, fs, io::Cursor, io::Write};

use sha2::{Digest, Sha256};

use super::{CacheDigest, CacheEntry, CachedIdentification};
use crate::local::cache::{
    CacheLinkPreference,
    hash::Blake3Hash,
    mods::{CacheStatus, CacheUnavailableReason, ModsCacheDir},
};
use anyhow::Result;

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
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let writer = &cache;
    let id = writer.import(&source, CacheLinkPreference::Copy).unwrap();
    let empty = write_identified(writer, id, &bytes, HashMap::new()).unwrap();
    assert!(!empty.digest_path().exists());

    let fabric = BUILTIN_LOADERS.get("fabric").unwrap();
    let metadata = fabric
        .instance()
        .try_identify(&ModEntry::new(source).unwrap())
        .unwrap();
    write_identified(
        writer,
        id,
        &bytes,
        [(fabric.id().clone(), metadata)].into_iter().collect(),
    )
    .unwrap();
    let entry = write_identified(
        writer,
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

    // Read through the cache and its codec owner before simulating an older parser version.
    let mut outdated = writer
        .cache
        .try_read_metadata::<CacheDigest>(&id, super::codec::METANAME)
        .unwrap()
        .unwrap();
    outdated.metadata_version = 0;
    writer
        .cache
        .write_metadata(&id, super::codec::METANAME, &outdated)
        .unwrap()
        .unwrap();
    let entry = write_identified(
        writer,
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
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let writer = &cache;
    let id = writer.import(&source, CacheLinkPreference::Copy).unwrap();
    let unrecognized = || [("forge".into(), None)].into_iter().collect();

    assert!(write_identified(writer, id, b"wrong bytes", unrecognized()).is_err());
    assert!(!writer.mod_metadata(id).digest_path().exists());
    let entry = write_identified(writer, id, &bytes, unrecognized()).unwrap();
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
            writer,
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
            writer,
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
    assert!(write_identified(writer, id, &bytes, unrecognized()).is_err());
    assert_eq!(fs::read(entry.digest_path()).unwrap(), original);
    writer.remove(&id).unwrap();
    assert!(write_identified(writer, id, &bytes, unrecognized()).is_err());
    assert!(!entry.digest_path().exists());
}

fn write_identified(
    writer: &ModsCacheDir,
    id: Blake3Hash,
    bytes: &[u8],
    metadata: HashMap<LoaderId, Option<ModMeta>>,
) -> Result<CacheEntry> {
    // Never expose or use write_metadata outside this crate under any circumstances:
    // incorrect associations destroy data consistency. Tests supply the identified
    // bytes explicitly; negative cases deliberately violate this contract.
    super::write_metadata(
        &writer.cache,
        id,
        &format!("{:x}", Sha256::digest(bytes)),
        &metadata,
    )
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
fn concurrent_metadata_publications_are_complete_valid_snapshots() {
    use super::{MOD_METADATA_VERSION, MetadataRefresh};
    use std::sync::{Arc, Barrier};

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("empty.jar");
    fs::write(&source, archive(&[])).unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = cache.import(&source, CacheLinkPreference::Copy).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let path = cache.path().to_owned();
    std::thread::scope(|scope| {
        for loader in ["fabric", "forge", "neoforge"] {
            let barrier = barrier.clone();
            let path = path.clone();
            scope.spawn(move || {
                let cache = ModsCacheDir::new(path).unwrap();
                let writer = &cache;
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
        let reader = &cache;
        for _ in 0..100 {
            let entry = reader.mod_metadata(id);
            match entry.status() {
                CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest) => {}
                CacheStatus::Available => {
                    let digest = entry.digest().unwrap();
                    assert_eq!(digest.metadata_version(), MOD_METADATA_VERSION);
                    assert!(digest.valid_metadata());
                    assert_eq!(digest.blake3, id);
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
    let entry = cache.mod_metadata(id);
    assert_eq!(entry.status(), &CacheStatus::Available);
    // We deliberately do not require the union of all three writers' results.
}
