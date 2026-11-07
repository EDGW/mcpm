//! Stash operation transport preserves batch gates, selector order and independent item failures.

use std::{fs, num::NonZeroUsize, path::PathBuf, sync::Arc};

use anyhow::Result;
use mcpm_backend::operations::{self, server::stash::*};
use mcpm_exec::{
    error::{self, RemoteError},
    messaging::Session,
    operation::{self, Operation},
};
use mcpm_lib::local::mc::server::{MinecraftServer, MinecraftServerMetadata};

struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        MinecraftServer::create(
            root.path(),
            MinecraftServerMetadata {
                version: 3,
                minecraft_version: "1.21.1".into(),
                loader: None,
                start: None,
            },
        )
        .unwrap();
        fs::create_dir_all(root.path().join("mods")).unwrap();
        fs::write(root.path().join("mods/模组.txt"), b"first").unwrap();
        Self { root }
    }
    fn path(&self) -> PathBuf {
        self.root.path().into()
    }
}
fn parallelism() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
fn client() -> Session {
    let (client, server) = tokio::io::duplex(128);
    let mut server = Session::new(server);
    tokio::spawn(async move {
        operation::serve(&mut server, Arc::new(operations::registry().unwrap()))
            .await
            .unwrap();
    });
    Session::new(client)
}
async fn run<O: Operation>(remote: Option<&Session>, input: O::Input) -> Result<O::Output> {
    match remote {
        Some(session) => operation::call::<O>(session, &input).await,
        None => operation::execute::<O>(input).await,
    }
}
async fn create(remote: Option<&Session>, fixture: &Fixture, name: &str) -> StashSnapshot {
    let mut stream = run::<Create>(
        remote,
        CreateInput {
            path: fixture.path(),
            name: name.into(),
            parallelism: parallelism(),
        },
    )
    .await
    .unwrap();
    let mut result = None;
    while let Some(event) = stream.next().await {
        if let CreateEvent::Created(stash) = event.unwrap() {
            result = Some(stash);
        }
    }
    result.unwrap()
}

#[tokio::test]
async fn local_and_session_batches_preserve_workspace_and_head_protection() {
    let session = client();
    for remote in [None, Some(&session)] {
        let fixture = Fixture::new();
        let first = create(remote, &fixture, "初始").await;
        fs::write(fixture.root.path().join("mods/模组.txt"), b"second").unwrap();
        let second = create(remote, &fixture, "second").await;
        let mut status = run::<Status>(
            remote,
            StatusInput {
                path: fixture.path(),
                parallelism: parallelism(),
            },
        )
        .await
        .unwrap();
        let mut compared = None;
        while let Some(event) = status.next().await {
            if let StatusEvent::Status(value) = event.unwrap() {
                compared = Some(value);
            }
        }
        let compared = compared.unwrap();
        assert_eq!(compared.head.unwrap().id, second.id);
        let compared = compared.comparison.unwrap();
        assert_eq!(compared.expected, compared.actual);
        let renamed = run::<Rename>(
            remote,
            RenameInput {
                path: fixture.path(),
                selector: Selector::Id(first.id),
                new_name: "renamed".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.backup_id, first.backup_id);
        assert_eq!(renamed.created_at, first.created_at);
        let info = run::<Info>(
            remote,
            InfoInput {
                path: fixture.path(),
                selectors: vec![
                    Selector::Name("renamed".into()),
                    Selector::Id(first.id),
                    Selector::Id(second.id),
                ],
                include_manifest: true,
            },
        )
        .await
        .unwrap();
        let entries: Vec<_> = info.entries.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.stash.id)
                .collect::<Vec<_>>(),
            [first.id, second.id]
        );
        assert_eq!(
            entries[0].manifest.as_ref().unwrap().files[0].name,
            "模组.txt"
        );
        assert_eq!(entries[0].manifest.as_ref().unwrap().files[0].size, 5);
        assert!(
            run::<Delete>(
                remote,
                DeleteInput {
                    path: fixture.path(),
                    selectors: vec![Selector::Id(first.id), Selector::Id(second.id)]
                }
            )
            .await
            .is_err()
        );
        assert!(
            fixture
                .root
                .path()
                .join(".mcpm/stash/index")
                .join(format!("{}.json", first.id))
                .is_file()
        );
        let mut switched = run::<Switch>(
            remote,
            SwitchInput {
                path: fixture.path(),
                selector: Selector::Id(first.id),
                force: false,
                parallelism: parallelism(),
            },
        )
        .await
        .unwrap();
        let mut result = None;
        while let Some(event) = switched.next().await {
            if let SwitchEvent::Switched { stash, changed } = event.unwrap() {
                result = Some((stash.id, changed));
            }
        }
        assert_eq!(result, Some((first.id, true)));
        assert_eq!(
            fs::read(fixture.root.path().join("mods/模组.txt")).unwrap(),
            b"first"
        );
        let deleted = run::<Delete>(
            remote,
            DeleteInput {
                path: fixture.path(),
                selectors: vec![Selector::Id(second.id), Selector::Name("second".into())],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            deleted
                .into_iter()
                .map(|entry| entry.result.unwrap().id)
                .collect::<Vec<_>>(),
            [second.id]
        );
        let list = run::<List>(
            remote,
            ListInput {
                path: fixture.path(),
            },
        )
        .await
        .unwrap();
        assert_eq!(list.head.unwrap(), Some(first.id));
        assert_eq!(
            list.entries
                .into_iter()
                .map(|entry| entry.unwrap().stash.id)
                .collect::<Vec<_>>(),
            [first.id]
        );
        assert!(fixture.root.path().join(".mcpm/stash/blob").is_dir());
    }
}

#[tokio::test]
async fn corrupt_head_and_lazy_manifests_do_not_hide_other_items_across_transport() {
    let session = client();
    for remote in [None, Some(&session)] {
        let fixture = Fixture::new();
        let first = create(remote, &fixture, "first").await;
        fs::write(fixture.root.path().join("mods/模组.txt"), b"second").unwrap();
        let second = create(remote, &fixture, "second").await;
        let manifest = fixture
            .root
            .path()
            .join(".mcpm/data")
            .join(first.backup_id.to_string())
            .join("manifest.mcbackup");
        // Broken non-HEAD manifests must not turn a lightweight list/info into a failure.
        fs::remove_file(manifest).unwrap();
        fs::write(fixture.root.path().join(".mcpm/stash/HEAD"), b"corrupt").unwrap();
        let list = run::<List>(
            remote,
            ListInput {
                path: fixture.path(),
            },
        )
        .await
        .unwrap();
        let head = list.head.unwrap_err().into_inner();
        assert_eq!(
            error::contains::<RemoteError>(head.as_ref()),
            remote.is_some()
        );
        assert!(list.entries.into_iter().all(|entry| entry.is_ok()));
        let selectors = vec![
            Selector::Id(first.id),
            Selector::Name("first".into()),
            Selector::Id(second.id),
        ];
        let info = run::<Info>(
            remote,
            InfoInput {
                path: fixture.path(),
                selectors: selectors.clone(),
                include_manifest: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            info.entries
                .into_iter()
                .map(|entry| entry.unwrap().stash.id)
                .collect::<Vec<_>>(),
            [first.id, second.id]
        );
        let info = run::<Info>(
            remote,
            InfoInput {
                path: fixture.path(),
                selectors: selectors.clone(),
                include_manifest: true,
            },
        )
        .await
        .unwrap();
        let mut entries = info.entries.into_iter();
        let error = entries.next().unwrap().unwrap_err().into_inner();
        assert_eq!(
            error::contains::<RemoteError>(error.as_ref()),
            remote.is_some()
        );
        assert_eq!(entries.next().unwrap().unwrap().stash.id, second.id);
        let mut stream = run::<Verify>(
            remote,
            VerifyInput {
                path: fixture.path(),
                selectors,
                parallelism: parallelism(),
            },
        )
        .await
        .unwrap();
        let mut head_failures = 0;
        let mut items = Vec::new();
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                VerifyEvent::Progress(_) => {}
                VerifyEvent::HeadFailed(error) => {
                    assert_eq!(
                        error::contains::<RemoteError>(error.into_inner().as_ref()),
                        remote.is_some()
                    );
                    head_failures += 1;
                }
                VerifyEvent::Item { selector, result } => items.push((selector, result)),
            }
        }
        assert_eq!(head_failures, 1);
        assert_eq!(items[0].0, Selector::Id(first.id));
        assert!(items[0].1.is_err());
        assert_eq!(items[1].1.as_ref().unwrap().id, second.id);
        assert_eq!(items.len(), 2);
        assert!(
            run::<Delete>(
                remote,
                DeleteInput {
                    path: fixture.path(),
                    selectors: vec![Selector::Id(second.id)]
                }
            )
            .await
            .is_err()
        );
        assert!(
            fixture
                .root
                .path()
                .join(".mcpm/stash/index")
                .join(format!("{}.json", second.id))
                .is_file()
        );
    }
}
