//! Regression coverage for bounded concurrency, ordered collection and cached failures/aliases.

use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::{collections::HashMap, sync::Arc};

use mcpm_backend::local::modloader::BUILTIN_LOADERS;

use super::ScanSession;

#[test]
fn normalized_duplicates_share_success_and_failure_but_disabled_alias_is_separate() {
    let root = tempfile::tempdir().unwrap();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let good = root.path().join("good.jar");
    std::fs::copy(fixture, &good).unwrap();
    let disabled = root.path().join("good.jar.disabled");
    std::fs::copy(&good, &disabled).unwrap();
    let bad = root.path().join("broken.jar");
    std::fs::write(&bad, b"invalid archive").unwrap();
    let missing = root.path().join("missing.jar");
    let paths = [
        good.clone(),
        root.path().join("./good.jar"),
        disabled,
        bad.clone(),
        bad,
        missing.clone(),
        missing.clone(),
        root.path().join("invalid.txt"),
    ];
    let environment = RuntimeEnvironment::new().with_mods_cache(None);
    let mut scan = ScanSession::new(BUILTIN_LOADERS.clone(), &environment).with_parallelism(3);
    let mut files = HashMap::new();
    let mut fresh = 0;
    scan.batch(
        &paths,
        || Ok(()),
        |index, result, first| {
            fresh += usize::from(first);
            files.insert(index, result.file.clone());
            if index >= 3 {
                assert!(result.failed());
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(fresh, 5);
    assert_eq!(scan.failed_count(), 3);
    assert!(Arc::ptr_eq(
        files[&0].as_ref().unwrap(),
        files[&1].as_ref().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        files[&0].as_ref().unwrap(),
        files[&2].as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        files[&5].as_ref().unwrap(),
        files[&6].as_ref().unwrap()
    ));
    assert!(files[&7].is_none());
    std::fs::copy(&good, &missing).unwrap();
    scan.batch(
        &paths,
        || Ok(()),
        |index, result, fresh| {
            assert!(!fresh);
            if index == 5 {
                assert!(result.failed());
            }
            Ok(())
        },
    )
    .unwrap();
}
