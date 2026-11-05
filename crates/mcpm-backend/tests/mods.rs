//! Host Mod selection, read-only cache reuse and metadata survive local and Session execution.

use anyhow::Result;
use mcpm_backend::operations::{
    self,
    mods::{
        info::Info,
        list::{Event as ListEvent, Input as ListInput, List, Selection as ListSelection},
        loading::{
            ScopeInput,
            inspection::{Event, Input},
            selection::SelectionInput,
        },
    },
};
use mcpm_exec::{
    error::{self, RemoteError},
    messaging::Session,
    operation::{self, Operation},
};
use mcpm_lib::local::{
    cache::{
        CacheLinkPreference,
        mods::{MetadataRefresh, ModsCacheDir, ModsCacheSelection},
    },
    modloader::BUILTIN_LOADERS,
};
use serde_json::json;
use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
};

fn session() -> Session {
    let (client, server) = tokio::io::duplex(1024);
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
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mcpm/tests/fixtures")
        .join(name)
}
fn selection(value: serde_json::Value) -> SelectionInput {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn selected_archive_subtrees_and_cached_failure_identity_survive_transport() -> Result<()> {
    let session = session();
    for remote in [None, Some(&session)] {
        let root = tempfile::tempdir()?;
        fs::copy(
            fixture("complex-mod.jar"),
            root.path().join("quoted ' mod.jar.disabled"),
        )?;
        fs::write(root.path().join("broken.jar"), b"invalid archive")?;
        let mut stream = run::<Info>(
            remote,
            Input {
                scope: ScopeInput::Directory {
                    directory: root.path().into(),
                    loaders: vec!["fabric".into()],
                    cache: None,
                },
                selection: selection(json!({"queries":[
                    {"kind":"modid","value":"omnibus_nested"},
                    {"kind":"file","value":"missing.jar"},
                    {"kind":"file","value":"missing.jar"},
                    {"kind":"file","value":"quoted ' mod.jar.disabled"}
                ]})),
                parallelism: NonZeroUsize::new(3).unwrap(),
            },
        )
        .await?;
        let mut files = Vec::new();
        let mut errors = 0;
        let mut done = None;
        while let Some(event) = stream.next().await {
            match event? {
                Event::File { file, .. } => files.push(file),
                Event::Error { error: failure } => {
                    assert_eq!(
                        error::contains::<RemoteError>(failure.as_inner().as_ref()),
                        remote.is_some()
                    );
                    errors += 1;
                }
                Event::Done { files, failed, .. } => done = Some((files, failed)),
                _ => {}
            }
        }
        assert_eq!(done, Some((4, 2)));
        assert_eq!(errors, 2);
        let records = &files[0].metadata[0].records;
        assert_eq!(
            records
                .iter()
                .map(|record| record.info.modid.as_str())
                .collect::<Vec<_>>(),
            ["omnibus_nested", "omnibus_leaf"]
        );
        assert_eq!(
            records[1].source,
            ["META-INF/jars/child.jar", "META-INF/jars/grandchild.jar"]
        );
        assert_eq!(files[3].filename, "quoted ' mod.jar.disabled");
        assert!(!files[3].enabled);
    }
    Ok(())
}

#[tokio::test]
async fn list_cache_counts_are_per_file_not_query_or_nested_row_and_do_not_publish_missing_metadata()
-> Result<()> {
    let session = session();
    for remote in [None, Some(&session)] {
        let root = tempfile::tempdir()?;
        let mods = root.path().join("mods");
        fs::create_dir(&mods)?;
        let cache = ModsCacheDir::initialize(root.path().join("cache"))?;
        let full = cache.import(&fixture("complex-mod.jar"), CacheLinkPreference::Copy)?;
        cache.refresh_metadata(full, &BUILTIN_LOADERS, MetadataRefresh::Force)?;
        cache.export_file(
            &full,
            &mods.join("full.jar.disabled"),
            CacheLinkPreference::HardLink,
        )?;
        let partial = cache.import(
            &fixture("dependency-problems/mods/fabric_only.jar"),
            CacheLinkPreference::Copy,
        )?;
        let fabric = BUILTIN_LOADERS
            .iter()
            .filter(|(id, _)| id.as_str() == "fabric")
            .map(|(id, loader)| (id.clone(), loader.clone()))
            .collect();
        cache.refresh_metadata(partial, &fabric, MetadataRefresh::Force)?;
        cache.export_file(
            &partial,
            &mods.join("partial.jar"),
            CacheLinkPreference::HardLink,
        )?;
        fs::copy(fixture("complex-mod.jar"), mods.join("copy.jar"))?;
        fs::write(mods.join("broken.jar"), b"invalid ZIP")?;
        let digest = cache.mod_metadata(partial).digest_path().to_owned();
        let before = fs::read(&digest)?;
        let mut stream = run::<List>(
            remote,
            ListInput {
                scope: ScopeInput::Directory {
                    directory: root.path().into(),
                    loaders: Vec::new(),
                    cache: Some(ModsCacheSelection::Directory {
                        path: cache.path().into(),
                    }),
                },
                selection: ListSelection::Paths(vec![mods.clone(), mods.join("full.jar.disabled")]),
                parallelism: NonZeroUsize::new(3).unwrap(),
            },
        )
        .await?;
        let mut counts = None;
        let mut nested = false;
        while let Some(event) = stream.next().await {
            match event? {
                ListEvent::File { file } => {
                    nested |= file
                        .scanned
                        .file
                        .metadata
                        .iter()
                        .any(|meta| !meta.nested.is_empty());
                }
                ListEvent::Done { cache, failed, .. } => {
                    counts = Some((
                        cache.fully_cached,
                        cache.partially_cached,
                        cache.total,
                        failed,
                    ))
                }
                _ => {}
            }
        }
        assert!(nested);
        assert_eq!(counts, Some((2, 1, 4, 1)));
        assert_eq!(fs::read(digest)?, before);
        assert_eq!(cache.list_metadata()?.len(), 2);
    }
    Ok(())
}

#[tokio::test]
async fn host_server_scope_rejects_a_remote_registration_without_touching_its_mod_directory()
-> Result<()> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join(".mcpm"))?;
    fs::write(
        root.path().join(".mcpm/mcpm.json"),
        br#"{"version":1,"type":"remote"}"#,
    )?;
    let metadata =
        br#"{"version":2,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"1.0"}}"#;
    fs::write(root.path().join(".mcpm/server.json"), metadata)?;
    let session = session();
    let result = run::<List>(
        Some(&session),
        ListInput {
            scope: ScopeInput::Server {
                path: root.path().into(),
                no_cache: true,
            },
            selection: ListSelection::Queries(selection(json!({"all":true}))),
            parallelism: NonZeroUsize::new(1).unwrap(),
        },
    )
    .await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("Host followed a Remote registration"),
    };
    assert!(error::contains::<RemoteError>(error.as_ref()));
    assert_eq!(fs::read(root.path().join(".mcpm/server.json"))?, metadata);
    assert!(!root.path().join("mods").exists());
    Ok(())
}
