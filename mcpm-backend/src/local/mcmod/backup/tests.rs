//! Behavioral coverage for semantic checksums, cache isolation and failed publications.

use super::*;
use crate::local::cache::mods::{ModsCacheDir, ModsCacheSelection};
use crate::local::files::FileIdentity;

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    backup: PathBuf,
    target: PathBuf,
    cache: ModsCacheDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("mods");
        fs::create_dir_all(source.join("nested/empty")).unwrap();
        fs::write(source.join("first.jar"), b"first content").unwrap();
        fs::write(source.join("nested/second.jar.disabled"), b"second content").unwrap();
        let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
        Self {
            source,
            backup: root.path().join("backup"),
            target: root.path().join("restored"),
            root,
            cache,
        }
    }
    fn create(&self, mode: CacheLinkPreference) -> Outcome {
        create(
            &self.source,
            &self.backup,
            &RuntimeEnvironment::new().with_backup_cache(ModsCacheSelection::Directory {
                path: self.cache.path().to_owned(),
            }),
            CreateOptions {
                mode,
                parallelism: NonZeroUsize::new(3).unwrap(),
            },
            |_| Ok(()),
        )
        .unwrap()
    }
    fn apply(&self, force: bool, mode: CacheLinkPreference) -> Result<Outcome> {
        apply(
            &self.backup,
            &self.target,
            ApplyOptions {
                force,
                mode,
                parallelism: NonZeroUsize::new(3).unwrap(),
            },
            |_| Ok(()),
        )
    }
    fn rewrite_manifest(&self, value: &serde_json::Value) {
        let path = self.backup.join(MANIFEST_NAME);
        fs::remove_file(&path).unwrap();
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Unlock test-owned directories, leaving linked file permissions untouched.
        let _ = files::remove_protected_tree(self.root.path());
    }
}

#[test]
fn hard_link_backup_is_content_only_and_restores_writable_copies_without_cache() {
    let fixture = Fixture::new();
    let outcome = fixture.create(CacheLinkPreference::HardLink);
    assert_eq!(
        outcome.manifest_size(),
        fs::metadata(fixture.backup.join(MANIFEST_NAME))
            .unwrap()
            .len()
    );
    assert_eq!(outcome.manifest().files().len(), 2);
    for (file, method) in outcome.files() {
        assert_eq!(method, TransferMethod::HardLink);
        let cached = fixture.cache.content_path(&file.blake3());
        assert_eq!(
            FileIdentity::read(&cached).unwrap(),
            FileIdentity::read(&fixture.backup.join("data").join(file.path())).unwrap()
        );
        assert!(
            fixture
                .cache
                .inspect(&file.blake3())
                .unwrap()
                .metadata
                .is_empty()
        );
        assert_ne!(
            FileIdentity::read(&cached).unwrap(),
            FileIdentity::read(&fixture.source.join(file.path())).unwrap()
        );
        fixture.cache.remove(&file.blake3()).unwrap();
    }
    let applied = fixture.apply(false, CacheLinkPreference::Copy).unwrap();
    for (file, method) in applied.files() {
        assert_eq!(method, TransferMethod::Copy);
        assert_eq!(
            fs::read(fixture.source.join(file.path())).unwrap(),
            fs::read(fixture.target.join(file.path())).unwrap()
        );
        assert_ne!(
            FileIdentity::read(&fixture.target.join(file.path())).unwrap(),
            FileIdentity::read(&fixture.backup.join("data").join(file.path())).unwrap()
        );
    }
    assert!(!fixture.backup.join("data/nested/empty").exists());
    assert!(!fixture.target.join("nested/empty").exists());
    fs::write(fixture.target.join("first.jar"), b"writable restored file").unwrap();
    fs::write(fixture.source.join("first.jar"), b"writable original file").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(fixture.backup.join("data"))
                .unwrap()
                .permissions()
                .mode()
                & 0o222,
            0
        );
        assert_eq!(
            fs::metadata(fixture.backup.join("data/first.jar"))
                .unwrap()
                .permissions()
                .mode()
                & 0o222,
            0
        );
        assert_eq!(
            fs::metadata(fixture.backup.join(MANIFEST_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o222,
            0
        );
    }
    #[cfg(windows)]
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.backup.join(MANIFEST_NAME))
            .is_err()
    );
}

#[test]
fn checksum_ignores_record_order_json_field_order_whitespace_and_escapes() {
    let fixture = Fixture::new();
    let outcome = fixture.create(CacheLinkPreference::Copy);
    let path = fixture.backup.join(MANIFEST_NAME);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["files"].as_array_mut().unwrap().reverse();
    fixture.rewrite_manifest(&value);
    let compact = fs::read_to_string(&path)
        .unwrap()
        .replace("first.jar", "\\u0066irst.jar");
    fs::write(&path, &compact).unwrap();
    assert_eq!(
        Manifest::read(&path).unwrap().checksum(),
        outcome.manifest().checksum()
    );
    let applied = fixture.apply(false, CacheLinkPreference::Copy).unwrap();
    assert_eq!(applied.manifest_size(), compact.len() as u64);
    value["files"][0]["path"] = "renamed.jar".into();
    fixture.rewrite_manifest(&value);
    assert!(Manifest::read(&path).unwrap_err().is::<ChecksumMismatch>());
    value["files"][0]["path"] = "../escape.jar".into();
    fixture.rewrite_manifest(&value);
    assert!(Manifest::read(&path).is_err());

    let valid = serde_json::to_string(&value).unwrap();
    fs::write(&path, valid.replacen('{', "{\"version\":1,", 1)).unwrap();
    assert!(
        Manifest::read(&path)
            .unwrap_err()
            .to_string()
            .contains("failed to decode")
    );
    value["unexpected"] = true.into();
    fixture.rewrite_manifest(&value);
    assert!(
        Manifest::read(&path)
            .unwrap_err()
            .to_string()
            .contains("failed to decode")
    );
}

#[cfg(unix)]
#[test]
fn corrupted_or_extra_backup_files_never_replace_force_target() {
    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("previous"), b"keep me").unwrap();
    // Replace a backup file, without changing the cached inode's protection.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.backup.join("data"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    #[cfg(unix)]
    {
        fs::write(fixture.backup.join("data/extra"), b"unlisted").unwrap();
        fs::remove_file(fixture.backup.join("data/first.jar")).unwrap();
        let error = fixture.apply(true, CacheLinkPreference::Copy).unwrap_err();
        match error.downcast_ref::<VerificationError>().unwrap() {
            VerificationError::FileSet {
                directory,
                missing,
                unexpected,
            } => {
                assert_eq!(
                    directory,
                    &fs::canonicalize(fixture.backup.join("data")).unwrap()
                );
                assert_eq!(missing, &["first.jar"]);
                assert_eq!(unexpected, &["extra"]);
            }
            failure => panic!("unexpected verification failure: {failure:?}"),
        }
        fs::remove_file(fixture.backup.join("data/extra")).unwrap();
        fs::write(fixture.backup.join("data/first.jar"), b"wrong content").unwrap();
        // Equal lengths must still fail complete BLAKE3 verification.
        let error = fixture.apply(true, CacheLinkPreference::Copy).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<VerificationError>(),
            Some(VerificationError::FileHash { .. })
        ));
        fs::write(fixture.backup.join("data/first.jar"), b"short").unwrap();
        let error = fixture.apply(true, CacheLinkPreference::Copy).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<VerificationError>(),
            Some(VerificationError::FileSize {
                expected: 13,
                actual: 5,
                ..
            })
        ));
    }
    assert_eq!(
        fs::read(fixture.target.join("previous")).unwrap(),
        b"keep me"
    );
}

#[test]
fn force_replaces_whole_directory_and_hard_link_apply_shares_backup_contents() {
    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("obsolete.jar"), b"obsolete").unwrap();
    assert!(fixture.apply(false, CacheLinkPreference::Copy).is_err());
    fixture.apply(true, CacheLinkPreference::HardLink).unwrap();
    assert!(!fixture.target.join("obsolete.jar").exists());
    assert_eq!(
        FileIdentity::read(&fixture.target.join("first.jar")).unwrap(),
        FileIdentity::read(&fixture.backup.join("data/first.jar")).unwrap()
    );
}

#[test]
fn failed_transfer_discards_staging_and_preserves_target() {
    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("previous"), b"keep").unwrap();
    let entries_before = fs::read_dir(fixture.root.path()).unwrap().count();
    let result = apply(
        &fixture.backup,
        &fixture.target,
        ApplyOptions {
            force: true,
            mode: CacheLinkPreference::Copy,
            parallelism: NonZeroUsize::new(2).unwrap(),
        },
        |event| {
            if matches!(event, Event::StageDone(Stage::Transfer)) {
                anyhow::bail!("consumer failed");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(fixture.target.join("previous")).unwrap(), b"keep");
    assert_eq!(
        fs::read_dir(fixture.root.path()).unwrap().count(),
        entries_before
    );
}

#[test]
fn empty_backups_and_invalid_manifest_structures_are_handled_without_directory_records() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.source.join("first.jar")).unwrap();
    fs::remove_file(fixture.source.join("nested/second.jar.disabled")).unwrap();
    let outcome = fixture.create(CacheLinkPreference::PreferHardLink);
    assert!(outcome.manifest().files().is_empty());
    assert_eq!(
        checksum(&fixture.source, NonZeroUsize::new(1).unwrap(), |_| Ok(()))
            .unwrap()
            .manifest()
            .checksum(),
        outcome.manifest().checksum()
    );
    fixture.apply(false, CacheLinkPreference::Copy).unwrap();
    assert_eq!(fs::read_dir(&fixture.target).unwrap().count(), 0);
    let key = crate::local::files::hash::blake3(b"x").unwrap();
    assert!(
        Manifest::new(vec![
            FileRecord::new("a".into(), key, 1),
            FileRecord::new("a/b".into(), key, 1)
        ])
        .is_err()
    );
    assert!(
        Manifest::new(vec![
            FileRecord::new("a".into(), key, 1),
            FileRecord::new("a".into(), key, 1)
        ])
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn links_and_overlapping_paths_are_rejected_before_publication() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("first.jar", fixture.source.join("linked.jar")).unwrap();
    assert!(checksum(&fixture.source, NonZeroUsize::new(1).unwrap(), |_| Ok(())).is_err());
    let options = CreateOptions {
        mode: CacheLinkPreference::Copy,
        parallelism: NonZeroUsize::new(1).unwrap(),
    };
    assert!(
        create(
            &fixture.source,
            &fixture.backup,
            &RuntimeEnvironment::new().with_backup_cache(ModsCacheSelection::Directory {
                path: fixture.cache.path().to_owned()
            }),
            options,
            |_| Ok(())
        )
        .is_err()
    );
    assert!(!fixture.backup.exists());
    fs::remove_file(fixture.source.join("linked.jar")).unwrap();
    assert!(
        create(
            &fixture.source,
            &fixture.source.join("backup"),
            &RuntimeEnvironment::new().with_backup_cache(ModsCacheSelection::Directory {
                path: fixture.cache.path().to_owned()
            }),
            options,
            |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn directory_checksum_matches_backup_after_relocation_without_cache_or_backup_data() {
    let fixture = Fixture::new();
    let created = fixture.create(CacheLinkPreference::Copy);
    for record in created.manifest().files() {
        fixture.cache.remove(&record.blake3()).unwrap();
    }
    files::remove_protected_tree(&fixture.backup.join("data")).unwrap();
    fs::rename(&fixture.source, &fixture.target).unwrap();
    fs::create_dir(fixture.target.join("another-empty-directory")).unwrap();
    let before = tree::scan(fixture.root.path()).unwrap().files;
    let computed = checksum(&fixture.target, NonZeroUsize::new(2).unwrap(), |_| Ok(())).unwrap();
    assert_eq!(
        computed.manifest().checksum(),
        created.manifest().checksum()
    );
    assert_eq!(
        computed.manifest().files()[1].path(),
        "nested/second.jar.disabled"
    );
    assert!(
        computed
            .compare_with_backup(&fixture.backup)
            .unwrap()
            .matches()
    );
    assert!(
        computed
            .compare_checksum(created.manifest().checksum())
            .matches()
    );
    assert!(verify(&fixture.backup, NonZeroUsize::new(1).unwrap(), |_| Ok(())).is_err());
    assert_eq!(tree::scan(fixture.root.path()).unwrap().files, before);
}

#[test]
fn directory_comparison_reports_all_path_and_content_differences_in_order() {
    let fixture = Fixture::new();
    let created = fixture.create(CacheLinkPreference::Copy);
    fs::remove_file(fixture.source.join("first.jar")).unwrap();
    fs::write(
        fixture.source.join("nested/second.jar.disabled"),
        b"changed bytes!",
    )
    .unwrap();
    fs::write(fixture.source.join("a.txt"), b"additional").unwrap();
    fs::write(fixture.source.join("z.txt"), b"additional").unwrap();
    let computed = checksum(&fixture.source, NonZeroUsize::new(2).unwrap(), |_| Ok(())).unwrap();
    let compared = computed.compare_with_backup(&fixture.backup).unwrap();
    assert!(!compared.matches());
    assert_eq!(compared.expected(), created.manifest().checksum());
    assert_eq!(compared.actual(), computed.manifest().checksum());
    assert!(matches!(compared.differences(), [
        FileDifference::Unexpected { actual: a },
        FileDifference::Missing { expected: missing },
        FileDifference::Changed { expected, actual },
        FileDifference::Unexpected { actual: z },
    ] if a.path() == "a.txt" && missing.path() == "first.jar"
        && expected.path() == "nested/second.jar.disabled" && expected.size() == actual.size()
        && expected.blake3() != actual.blake3() && z.path() == "z.txt"));
    assert!(
        computed
            .compare_checksum(created.manifest().checksum())
            .differences()
            .is_empty()
    );

    fs::write(fixture.source.join("nested/second.jar.disabled"), b"short").unwrap();
    let computed = checksum(&fixture.source, NonZeroUsize::new(1).unwrap(), |_| Ok(())).unwrap();
    let compared = computed.compare_manifest(created.manifest());
    assert!(matches!(&compared.differences()[2],
        FileDifference::Changed { expected, actual } if expected.size() != actual.size()));
}

#[test]
fn directory_checksum_rejects_inventory_changes_before_completion() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("original"), b"content").unwrap();
    let result = checksum(root.path(), NonZeroUsize::new(1).unwrap(), |event| {
        if matches!(
            event,
            Event::Progress {
                stage: Stage::Checksum,
                completed: 1,
                ..
            }
        ) {
            fs::write(root.path().join("added-during-hashing"), b"new")?;
        }
        Ok(())
    });
    assert!(
        matches!(result.unwrap_err().downcast_ref::<VerificationError>(),
        Some(VerificationError::FileSet { unexpected, .. }) if unexpected == &["added-during-hashing"])
    );
}

#[test]
fn publication_failure_restores_original_directory_and_target_changes_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("previous"), b"preserved").unwrap();
    let destination = tree::Destination::inspect(&target, true).unwrap();
    let holding = root.path().join("previous");
    assert!(
        destination
            .commit(&root.path().join("missing-stage"), &holding)
            .is_err()
    );
    assert!(!holding.exists());
    assert_eq!(fs::read(target.join("previous")).unwrap(), b"preserved");
    fs::rename(&target, root.path().join("moved-target")).unwrap();
    fs::create_dir(&target).unwrap();
    let stage = root.path().join("stage");
    fs::create_dir(&stage).unwrap();
    assert!(destination.commit(&stage, &holding).is_err());
    assert!(stage.exists());
    assert_eq!(
        fs::read(root.path().join("moved-target/previous")).unwrap(),
        b"preserved"
    );
}

#[test]
fn verify_is_read_only_and_works_after_cache_removal() {
    let fixture = Fixture::new();
    let created = fixture.create(CacheLinkPreference::HardLink);
    for file in created.manifest().files() {
        fixture.cache.remove(&file.blake3()).unwrap();
    }
    let before = tree::scan(fixture.root.path()).unwrap().files;
    let identity = FileIdentity::read(&fixture.backup.join("data/first.jar")).unwrap();
    let permissions = fs::metadata(fixture.backup.join("data/first.jar"))
        .unwrap()
        .permissions();
    let mut events = Vec::new();
    let verified = verify(&fixture.backup, NonZeroUsize::new(2).unwrap(), |event| {
        events.push(event);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        verified.manifest().checksum(),
        created.manifest().checksum()
    );
    assert_eq!(verified.manifest_size(), created.manifest_size());
    assert_eq!(tree::scan(fixture.root.path()).unwrap().files, before);
    assert_eq!(
        FileIdentity::read(&fixture.backup.join("data/first.jar")).unwrap(),
        identity
    );
    assert_eq!(
        fs::metadata(fixture.backup.join("data/first.jar"))
            .unwrap()
            .permissions(),
        permissions
    );
    assert!(!fixture.target.exists());
    assert!(events.iter().all(|event| matches!(
        event,
        Event::Progress {
            stage: Stage::Verify,
            ..
        } | Event::StageDone(Stage::Verify)
    )));
}

#[test]
fn deleting_a_protected_backup_preserves_cached_and_applied_hard_links() {
    let fixture = Fixture::new();
    let created = fixture.create(CacheLinkPreference::HardLink);
    fixture.apply(false, CacheLinkPreference::HardLink).unwrap();
    let before: Vec<_> = created
        .manifest()
        .files()
        .iter()
        .map(|file| {
            let cached = fixture.cache.content_path(&file.blake3());
            (
                file,
                FileIdentity::read(&cached).unwrap(),
                fs::metadata(&cached).unwrap().permissions(),
            )
        })
        .collect();

    delete(&fixture.backup).unwrap();

    assert!(!fixture.backup.exists());
    for (file, identity, permissions) in before {
        let cached = fixture.cache.content_path(&file.blake3());
        let applied = fixture.target.join(file.path());
        assert_eq!(FileIdentity::read(&cached).unwrap(), identity);
        assert_eq!(FileIdentity::read(&applied).unwrap(), identity);
        assert_eq!(fs::metadata(&cached).unwrap().permissions(), permissions);
        assert_eq!(fs::metadata(&applied).unwrap().permissions(), permissions);
        let original = fs::read(fixture.source.join(file.path())).unwrap();
        assert_eq!(fs::read(&cached).unwrap(), original);
        assert_eq!(fs::read(&applied).unwrap(), original);
        #[cfg(windows)]
        assert!(fs::OpenOptions::new().write(true).open(&cached).is_err());
    }
}

#[test]
fn deletion_refuses_unrecognized_roots_and_unrelated_entries_before_mutating_data() {
    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    let data = fixture.backup.join("data");
    let permissions = fs::metadata(&data).unwrap().permissions();
    fs::write(fixture.backup.join("keep.txt"), b"unrelated file").unwrap();
    assert!(delete(&fixture.backup).is_err());
    assert_eq!(
        fs::read(fixture.backup.join("keep.txt")).unwrap(),
        b"unrelated file"
    );
    assert_eq!(fs::metadata(&data).unwrap().permissions(), permissions);
    verify(&fixture.backup, NonZeroUsize::new(1).unwrap(), |_| Ok(())).unwrap();
    fs::remove_file(fixture.backup.join("keep.txt")).unwrap();

    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.backup.join(MANIFEST_NAME)).unwrap()).unwrap();
    value["files"][0]["size"] = 999.into();
    fixture.rewrite_manifest(&value);
    assert!(
        delete(&fixture.backup)
            .unwrap_err()
            .is::<ChecksumMismatch>()
    );
    assert_eq!(fs::read(data.join("first.jar")).unwrap(), b"first content");
    assert_eq!(fs::metadata(&data).unwrap().permissions(), permissions);

    assert!(delete(&fixture.source).is_err());
    assert_eq!(
        fs::read(fixture.source.join("first.jar")).unwrap(),
        b"first content"
    );
}

#[test]
fn deletion_accepts_missing_data_for_cleanup_after_partial_removal() {
    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    files::remove_protected_tree(&fixture.backup.join("data")).unwrap();
    delete(&fixture.backup).unwrap();
    assert!(!fixture.backup.exists());
}

#[cfg(unix)]
#[test]
fn deleting_damaged_data_unlinks_symlinks_without_touching_external_targets() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    fixture.create(CacheLinkPreference::Copy);
    let alias = fixture.root.path().join("backup-link");
    symlink(&fixture.backup, &alias).unwrap();
    assert!(delete(&alias).is_err());
    assert!(delete(&alias.join("")).is_err());
    assert!(delete(&alias.join(".")).is_err());
    assert!(fixture.backup.join(MANIFEST_NAME).exists());
    fs::remove_file(&alias).unwrap();
    let data = fixture.backup.join("data");
    files::remove_protected_tree(&data).unwrap();
    // Even a top-level data symlink is refused before permissions are changed.
    symlink(&fixture.source, &data).unwrap();
    assert!(delete(&fixture.backup).is_err());
    fs::remove_file(&data).unwrap();
    fs::create_dir(&data).unwrap();
    fs::write(data.join("unexpected"), b"not listed in manifest").unwrap();
    symlink(&fixture.source, data.join("external")).unwrap();
    symlink(fixture.source.join("first.jar"), data.join("external-file")).unwrap();
    symlink(fixture.source.join("missing"), data.join("dangling")).unwrap();
    let identity = FileIdentity::read(&fixture.source.join("first.jar")).unwrap();
    let permissions = fs::metadata(&fixture.source).unwrap().permissions();

    delete(&fixture.backup).unwrap();

    assert!(!fixture.backup.exists());
    assert_eq!(
        fs::read(fixture.source.join("first.jar")).unwrap(),
        b"first content"
    );
    assert_eq!(
        FileIdentity::read(&fixture.source.join("first.jar")).unwrap(),
        identity
    );
    assert_eq!(
        fs::metadata(&fixture.source).unwrap().permissions(),
        permissions
    );
}
