//! Scheduler behaviour that does not need a provider request: verified cache reuse and
//! per-source construction failures.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use kako_mc_lib::source::{
    FileHash, FileId, FileMatch, FileRef, FileSummary, MatchedFileRole, ProjectId, ProjectRef,
    SourceId,
};
use mcpm_exec::{LogLevel, Settings, Target};

use super::*;

fn session() -> crate::Session {
    let settings = Settings::new("test", LogLevel::Error, Arc::new(|_| {}));
    crate::Session::new(
        crate::executor(crate::SurfaceEnv::new().unwrap(), settings),
        Target::Local,
    )
}

fn summary() -> FileSummary {
    FileSummary {
        size: 3,
        sha1: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        murmur2: None,
    }
}

fn cached_match() -> FileMatch {
    FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId("modrinth".into()),
                id: ProjectId("project".into()),
            },
            id: FileId("file".into()),
        },
        filename: "create.jar".into(),
        size: 3,
        hashes: vec![FileHash::Sha1(summary().sha1)],
        role: MatchedFileRole::Primary,
    }
}

fn file(cached: BTreeMap<String, FileMatch>) -> IdentifyFile {
    IdentifyFile {
        indices: vec![0],
        path: PathBuf::from("create.jar"),
        summary: summary(),
        cached,
    }
}

#[tokio::test]
async fn profiles_report_cache_keys_and_per_source_failures() {
    let session = session();
    let mut handle = session
        .start::<Profiles>(ProfilesInput {
            specifications: vec!["modrinth".into(), "not-a-source".into()],
        })
        .await
        .unwrap();
    while handle.next().await.is_some() {}
    let output = handle.finish().await.unwrap();
    assert_eq!(output.profiles.len(), 2);
    assert_eq!(output.profiles[0].cache_key, "modrinth");
    assert!(output.profiles[0].error.is_none());
    assert!(!output.profiles[0].requirements.murmur2);
    assert_eq!(output.profiles[1].cache_key, "not-a-source");
    assert!(output.profiles[1].error.is_some());
}

#[tokio::test]
async fn verified_cache_hit_skips_the_provider() {
    let session = session();
    let mut cached = BTreeMap::new();
    cached.insert("modrinth".to_owned(), cached_match());
    let mut handle = session
        .start::<Identify>(IdentifyInput {
            specifications: vec!["modrinth".into()],
            files: vec![file(cached)],
            parallelism: 1,
        })
        .await
        .unwrap();
    let mut cache_hits = Vec::new();
    let mut matched = 0;
    while let Some(event) = handle.next().await {
        match event {
            IdentifyEvent::Cache { indices, sources } => {
                assert_eq!(indices, vec![0]);
                cache_hits = sources;
            }
            IdentifyEvent::Item {
                outcome: IdentifyOutcome::Matched { file, cache },
                ..
            } => {
                assert_eq!(file.reference.project.source, SourceId("modrinth".into()));
                assert_eq!(*cache, cached_match());
                matched += 1;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert_eq!(cache_hits, vec![0]);
    assert_eq!(matched, 1);
    let output = handle.finish().await.unwrap();
    assert_eq!(output.failed, 0);
}

#[tokio::test]
async fn an_unusable_source_fails_only_its_items() {
    let session = session();
    let mut handle = session
        .start::<Identify>(IdentifyInput {
            specifications: vec!["not-a-source".into()],
            files: vec![file(BTreeMap::new())],
            parallelism: 1,
        })
        .await
        .unwrap();
    let mut failed = 0;
    while let Some(event) = handle.next().await {
        if let IdentifyEvent::Item {
            outcome: IdentifyOutcome::Failed { .. },
            ..
        } = event
        {
            failed += 1;
        }
    }
    assert_eq!(failed, 1);
    let output = handle.finish().await.unwrap();
    assert_eq!(output.failed, 1);
}
