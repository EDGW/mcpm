//! Cache execution and Session transport preserve frozen inventories, partial writes and protection.

use std::{fs, num::NonZeroUsize, path::Path, sync::Arc};

use anyhow::Result;
use mcpm_backend::operations::{self, cache::*};
use mcpm_exec::{
    error::{self, RemoteError},
    messaging::Session,
    operation::{self, Operation},
};
use mcpm_lib::local::cache::{
    CacheLinkPreference,
    hash::Blake3Hash,
    mods::{ModsCacheDir, ModsCacheSelection},
};

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
async fn run<O: Operation>(session: Option<&Session>, input: O::Input) -> Result<O::Output> {
    match session {
        Some(session) => operation::call::<O>(session, &input).await,
        None => operation::execute::<O>(input).await,
    }
}
fn input(path: &Path) -> CacheInput {
    CacheInput {
        selection: ModsCacheSelection::Directory { path: path.into() },
        no_protect_stash: false,
    }
}

#[tokio::test]
async fn frozen_inventory_and_partial_receipts_survive_local_and_session_execution() -> Result<()> {
    let session = client();
    for remote in [None, Some(&session)] {
        let root = tempfile::tempdir()?;
        let config = input(&root.path().join("cache"));
        let initialized = run::<Init>(remote, config.clone()).await?;
        let cache = ModsCacheDir::new(&initialized.directory)?;
        let first = root.path().join("first.jar");
        fs::write(&first, b"first contents")?;
        let mut importing = run::<Import>(
            remote,
            ImportInput {
                cache: config.clone(),
                paths: vec![first, root.path().join("missing.jar")],
                mode: TransferPreference::Copy,
            },
        )
        .await?;
        let receipt = importing.next().await.unwrap()?;
        assert!(receipt.published);
        assert_eq!(fs::read(&receipt.file)?, b"first contents");
        let failure = importing.next().await.unwrap().unwrap_err();
        assert_eq!(
            error::contains::<RemoteError>(failure.as_ref()),
            remote.is_some()
        );
        assert!(importing.next().await.is_none());
        let inventory = run::<List>(remote, config.clone()).await?;
        assert_eq!(inventory.directory, initialized.directory);
        assert_eq!(
            inventory
                .entries
                .iter()
                .map(|entry| entry.key)
                .collect::<Vec<_>>(),
            [receipt.id]
        );

        let retained = root.path().join("retained.jar");
        let skipped = root.path().join("skipped.jar");
        let mut exporting = run::<Export>(
            remote,
            ExportInput {
                cache: config.clone(),
                id: receipt.id,
                destinations: vec![retained.clone(), retained.clone(), skipped.clone()],
                mode: TransferPreference::Copy,
            },
        )
        .await?;
        assert_eq!(exporting.next().await.unwrap()?.destination, retained);
        assert!(exporting.next().await.unwrap().is_err());
        assert_eq!(fs::read(&retained)?, b"first contents");
        assert!(!skipped.exists());

        let later = root.path().join("later.jar");
        fs::write(&later, b"added after inventory")?;
        let later = cache.import(&later, CacheLinkPreference::Copy)?;
        let mut removal = run::<Remove>(
            remote,
            RemoveInput {
                cache: config,
                directory: Some(inventory.directory),
                ids: inventory
                    .entries
                    .into_iter()
                    .map(|entry| entry.key)
                    .collect(),
            },
        )
        .await?;
        let removed = removal.next().await.unwrap()?;
        assert_eq!(removed.id, receipt.id);
        assert!(removed.removed);
        assert!(removal.next().await.is_none());
        assert_eq!(cache.list()?, [later]);
        assert_eq!(fs::read(&retained)?, b"first contents");
    }
    Ok(())
}

#[tokio::test]
async fn inventory_observation_and_verification_keep_metadata_and_content_independent() -> Result<()>
{
    let session = client();
    for remote in [None, Some(&session)] {
        let root = tempfile::tempdir()?;
        let cache = ModsCacheDir::initialize(root.path().join("cache"))?;
        let source = root.path().join("source.jar");
        fs::write(&source, b"not a Mod archive")?;
        let valid = cache.import(&source, CacheLinkPreference::Copy)?;
        let metadata = cache.ident_info(valid).path().to_owned();
        fs::write(&metadata, b"invalid metadata JSON")?;
        let config = input(cache.path());
        let inventory = run::<List>(remote, config.clone()).await?;
        let structural = inventory
            .entries
            .into_iter()
            .next()
            .unwrap()
            .into_entry(true)?;
        assert_eq!(structural.status, ContentStatus::Available);
        assert_eq!(structural.metadatas[0].name(), "ident");
        let mut information = run::<Info>(
            remote,
            InfoInput {
                cache: config.clone(),
                selection: Selection::Ids(vec![valid]),
                options: ReadOptions {
                    metadata: true,
                    metadata_details: true,
                    sha256: true,
                },
            },
        )
        .await?;
        let observed = information.next().await.unwrap()?;
        assert_eq!(observed.status, ContentStatus::Available);
        assert_eq!(observed.sha256, None);
        let Metadata::Ident(ident) = &observed.metadatas[0] else {
            panic!("expected ident observation")
        };
        assert_eq!(ident.state.status, ContentStatus::Unavailable);
        assert!(information.next().await.is_none());
        assert_eq!(fs::read(&metadata)?, b"invalid metadata JSON");

        let missing = Blake3Hash::new([1; 32]);
        let directory = Blake3Hash::new([2; 32]);
        fs::create_dir_all(cache.content_path(&directory))?;
        let mut verifying = run::<Verify>(
            remote,
            VerifyInput {
                cache: config,
                selection: Selection::Ids(vec![missing, directory, valid]),
                parallelism: NonZeroUsize::new(3).unwrap(),
                structure_only: false,
                autoremove: true,
            },
        )
        .await?;
        assert!(matches!(
            verifying.next().await.unwrap()?,
            VerifyEvent::Selected { total: 3 }
        ));
        let mut results = Vec::new();
        let mut summary = None;
        while let Some(event) = verifying.next().await {
            match event? {
                VerifyEvent::Item(mut item) => {
                    if let Some(error) = item.error.take() {
                        assert_eq!(
                            error::contains::<RemoteError>(error.into_inner().as_ref()),
                            remote.is_some()
                        );
                    }
                    results.push(item);
                }
                VerifyEvent::Finished(result) => summary = Some(result),
                VerifyEvent::Selected { .. } => panic!("duplicate selection"),
            }
        }
        assert_eq!(
            results.iter().map(|item| item.id).collect::<Vec<_>>(),
            [missing, directory, valid]
        );
        assert_eq!(results[0].status, ContentStatus::NotFound);
        assert!(matches!(results[0].removal, Some(Ok(false))));
        assert_eq!(results[1].status, ContentStatus::Unavailable);
        assert!(matches!(results[1].removal, Some(Err(_))));
        assert_eq!(results[2].status, ContentStatus::Available);
        let summary = summary.unwrap();
        assert_eq!((summary.available, summary.failed), (1, 2));
        let removal = summary.removal.unwrap();
        assert_eq!((removal.removed, removal.absent, removal.failed), (0, 1, 1));
        assert_eq!(cache.verified_size(&valid)?, Some(17));
        assert!(cache.content_path(&directory).is_dir());
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn host_preflight_checks_every_export_target_and_original_stash_alias() -> Result<()> {
    use mcpm_lib::local::mc::server::{MinecraftServer, MinecraftServerMetadata};
    use std::os::unix::fs::symlink;
    let session = client();
    for remote in [None, Some(&session)] {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("server"))?;
        let server = MinecraftServer::create(
            root.path().join("server"),
            MinecraftServerMetadata {
                version: 3,
                minecraft_version: "1.21.1".into(),
                loader: None,
                start: None,
            },
        )?;
        let blob = server.runtime().cache.stash.selection().path()?;
        let cache = ModsCacheDir::initialize(root.path().join("ordinary-cache"))?;
        fs::create_dir_all(blob.parent().unwrap())?;
        symlink(cache.path(), &blob)?;
        let source = root.path().join("source.jar");
        fs::write(&source, b"protected")?;
        let id = cache.import(&source, CacheLinkPreference::Copy)?;
        let first = root.path().join("first.jar");
        let protected = blob.join("injected.jar");
        let mut exporting = run::<Export>(
            remote,
            ExportInput {
                cache: input(cache.path()),
                id,
                destinations: vec![first.clone(), protected.clone()],
                mode: TransferPreference::Copy,
            },
        )
        .await?;
        assert!(exporting.next().await.unwrap().is_err());
        assert!(!first.exists());
        assert!(!protected.exists());
        let mut removal = run::<Remove>(
            remote,
            RemoveInput {
                cache: input(&blob),
                directory: Some(cache.path().into()),
                ids: vec![id],
            },
        )
        .await?;
        assert!(removal.next().await.unwrap().is_err());
        assert_eq!(cache.verified_size(&id)?, Some(9));
    }
    Ok(())
}
