//! Stash state transitions, destructive-operation gates, broken metadata and process lease behavior.

use super::*;
use crate::local::{
    files,
    mc::server::{MinecraftServerMetadata, ServerModLoader},
};

struct Fixture {
    root: tempfile::TempDir,
    server: MinecraftServer,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let server = MinecraftServer::create(
            root.path(),
            MinecraftServerMetadata {
                version: 2,
                minecraft_version: "1.21.1".into(),
                loader: Some(ServerModLoader {
                    loader_type: "fabric".into(),
                    version: "1".into(),
                }),
                start: None,
            },
        )
        .unwrap();
        fs::write(root.path().join("mods/mod.jar"), b"original").unwrap();
        fs::write(root.path().join("mods/note.txt"), b"also snapshotted").unwrap();
        Self { root, server }
    }

    fn parallelism(&self) -> NonZeroUsize {
        NonZeroUsize::new(2).unwrap()
    }

    fn change(&self, content: &[u8]) {
        fs::write(self.root.path().join("mods/mod.jar"), content).unwrap();
    }

    fn contents(&self) -> Vec<u8> {
        fs::read(self.root.path().join("mods/mod.jar")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = files::remove_protected_tree(self.root.path());
    }
}

#[test]
fn baseline_switch_and_batch_deletion_protect_unsaved_and_current_versions() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let mut store = StashStore::write(&fixture.server).unwrap();
    assert_eq!(
        store.status(n, |_| Ok(())).unwrap().state(),
        WorkspaceState::Untracked
    );
    let original = store.create("original".into(), n, |_| Ok(())).unwrap();
    assert!(store.create("duplicate".into(), n, |_| Ok(())).is_err());
    fixture.change(b"updated");
    let updated = store.create("updated".into(), n, |_| Ok(())).unwrap();
    fixture.change(b"unsaved");
    assert!(
        store
            .switch(&Selector::Id(original.id()), false, n, |_| Ok(()))
            .is_err()
    );
    assert_eq!(fixture.contents(), b"unsaved");
    assert_eq!(store.head().unwrap().unwrap().id(), updated.id());
    assert!(
        store
            .switch(&Selector::Id(updated.id()), true, n, |_| Ok(()))
            .unwrap()
            .changed
    );
    assert_eq!(fixture.contents(), b"updated");
    assert!(
        !store
            .switch(&Selector::Id(updated.id()), false, n, |_| Ok(()))
            .unwrap()
            .changed
    );
    assert!(
        store
            .delete(&[Selector::Id(original.id()), Selector::Id(updated.id())])
            .is_err()
    );
    assert!(store.resolve(&Selector::Id(original.id())).is_ok());
    assert!(store.backup_path(&original).exists());
    let renamed = store
        .rename(&Selector::Id(updated.id()), "renamed".into())
        .unwrap();
    assert_eq!(renamed.created_at(), updated.created_at());
    assert_eq!(renamed.backup_id(), updated.backup_id());
    assert!(
        store
            .rename(&Selector::Id(original.id()), "renamed".into())
            .is_err()
    );
    store
        .switch(&Selector::Id(original.id()), false, n, |_| Ok(()))
        .unwrap();
    assert_eq!(fixture.contents(), b"original");
    assert_eq!(
        fs::read(fixture.root.path().join("mods/note.txt")).unwrap(),
        b"also snapshotted"
    );
    // Source workspaces are writable copies, not protected links to saved content.
    fixture.change(b"independently writable");
    store
        .verify(&Selector::Id(original.id()), n, |_| Ok(()))
        .unwrap();
    assert_eq!(
        store
            .delete(&[Selector::Id(updated.id()), Selector::Name("renamed".into())])
            .unwrap()
            .len(),
        1
    );
    assert!(fixture.root.path().join(".mcpm/stash/blob").exists());
    // Restoring is independent of the cache once backups have been created.
    files::remove_protected_tree(&fixture.root.path().join(".mcpm/stash/blob")).unwrap();
    store
        .switch(&Selector::Id(original.id()), true, n, |_| Ok(()))
        .unwrap();
    assert_eq!(fixture.contents(), b"original");
}

#[test]
fn target_verification_precedes_force_and_already_current_shortcuts() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let mut store = StashStore::write(&fixture.server).unwrap();
    let stash = store.create("saved".into(), n, |_| Ok(())).unwrap();
    let data = store.backup_path(&stash).join("data");
    let _protection = files::DirectoryProtection::allow_removal(&data).unwrap();
    fs::write(data.join("unexpected"), b"invalid backup inventory").unwrap();
    // Even a clean, already-current target must not bypass verify.
    assert!(
        store
            .switch(&Selector::Id(stash.id()), false, n, |_| Ok(()))
            .is_err()
    );
    fixture.change(b"unsaved");
    assert!(
        store
            .switch(&Selector::Id(stash.id()), true, n, |_| Ok(()))
            .is_err()
    );
    assert_eq!(fixture.contents(), b"unsaved");
    assert_eq!(store.head().unwrap().unwrap().id(), stash.id());
}

#[test]
fn broken_head_blocks_destructive_policy_but_not_independent_verification() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let mut store = StashStore::write(&fixture.server).unwrap();
    let stash = store.create("saved".into(), n, |_| Ok(())).unwrap();
    fs::write(fixture.root.path().join(".mcpm/stash/HEAD"), b"broken json").unwrap();
    fixture.change(b"unsaved");
    assert!(
        store
            .switch(&Selector::Id(stash.id()), true, n, |_| Ok(()))
            .is_err()
    );
    assert!(store.create("another".into(), n, |_| Ok(())).is_err());
    assert!(store.delete(&[Selector::Id(stash.id())]).is_err());
    assert_eq!(fixture.contents(), b"unsaved");
    store
        .verify(&Selector::Id(stash.id()), n, |_| Ok(()))
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn successful_apply_with_failed_head_publication_reports_files_already_switched() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let mut store = StashStore::write(&fixture.server).unwrap();
    let original = store.create("original".into(), n, |_| Ok(())).unwrap();
    fixture.change(b"updated");
    store.create("updated".into(), n, |_| Ok(())).unwrap();
    let head = fixture.root.path().join(".mcpm/stash/HEAD");
    let result = store.switch(&Selector::Id(original.id()), false, n, |event| {
        if matches!(event, backup::Event::StageDone(backup::Stage::Transfer)) {
            fs::remove_file(&head)?;
            fs::create_dir(&head)?;
        }
        Ok(())
    });
    let error = result.err().expect("HEAD publication must fail");
    assert!(error.to_string().contains("Mod files were switched"));
    assert_eq!(fixture.contents(), b"original");
    assert!(head.is_dir());
    store
        .verify(&Selector::Id(original.id()), n, |_| Ok(()))
        .unwrap();
}

#[test]
fn failed_publication_and_cleanup_retain_independent_causes_and_backup_location() {
    let fixture = Fixture::new();
    let mut store = StashStore::write(&fixture.server).unwrap();
    let result = store.create("saved".into(), fixture.parallelism(), |event| {
        if matches!(event, backup::Event::StageDone(backup::Stage::Transfer)) {
            // Fail index publication and make the newly staged backup unsafe to delete.
            fs::write(
                fixture.root.path().join(".mcpm/stash/index"),
                b"blocks index directory",
            )?;
            let data = fixture.root.path().join(".mcpm/data");
            let workspace = fs::read_dir(data)?.next().unwrap()?.path();
            fs::write(
                workspace.join("new/unexpected-entry"),
                b"unrecognized backup root",
            )?;
        }
        Ok(())
    });
    let error = result.err().expect("publication must fail");
    let failure = error
        .downcast_ref::<CreateCleanupError>()
        .expect("both errors must be retained");
    assert!(
        error
            .chain()
            .any(|cause| cause.to_string().contains("expected a directory"))
    );
    assert!(
        failure
            .cleanup()
            .chain()
            .any(|cause| cause.to_string().contains("unexpected top-level entry"))
    );
    assert!(failure.backup().join(backup::MANIFEST_NAME).is_file());
    assert!(store.index.head().unwrap().is_none());
}

#[test]
fn invalid_shared_backup_reference_cannot_delete_the_heads_data() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let mut store = StashStore::write(&fixture.server).unwrap();
    let original = store.create("original".into(), n, |_| Ok(())).unwrap();
    fixture.change(b"updated");
    let head = store.create("updated".into(), n, |_| Ok(())).unwrap();
    let path = fixture
        .root
        .path()
        .join(".mcpm/stash/index")
        .join(format!("{}.json", original.id()));
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["backup"] = serde_json::Value::String(head.backup_id().to_string());
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let results = store.delete(&[Selector::Id(original.id())]).unwrap();
    assert!(results[0].result.is_err());
    store
        .verify(&Selector::Id(head.id()), n, |_| Ok(()))
        .unwrap();
    assert!(path.exists());
}

#[test]
fn empty_read_snapshot_and_shared_leases_serialize_first_publication() {
    let fixture = Fixture::new();
    let n = fixture.parallelism();
    let empty = StashStore::read(&fixture.server).unwrap();
    assert!(!fixture.root.path().join(".mcpm/stash").exists());
    let mut writer = StashStore::write(&fixture.server).unwrap();
    writer.create("saved".into(), n, |_| Ok(())).unwrap();
    assert!(empty.list().unwrap().is_empty());
    assert!(empty.head().unwrap().is_none());
    assert!(StashStore::read(&fixture.server).is_err());
    drop(writer);
    let reader = StashStore::read(&fixture.server).unwrap();
    let another_reader = StashStore::read(&fixture.server).unwrap();
    assert!(StashStore::write(&fixture.server).is_err());
    drop(reader);
    drop(another_reader);
    assert!(StashStore::write(&fixture.server).is_ok());
}
