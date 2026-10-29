//! Independent source snapshots, canonical provider identities and verified publication boundaries.

use anyhow::Result;
use async_trait::async_trait;
use mcpm_backend::local::cache::mods::CacheStatus;
use mcpm_backend::local::cache::mods::CacheUnavailableReason;
use mcpm_backend::local::cache::mods::MetadataRefresh;
use mcpm_backend::local::cache::mods::ModsCacheDir;
use mcpm_backend::local::cache::mods::ident::RefreshStatus;
use mcpm_backend::source::*;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

struct TestSource {
    id: SourceId,
    key: SourceId,
    calls: AtomicUsize,
    answer: u8,
}
impl TestSource {
    fn new(id: &str, key: &str, answer: u8) -> Self {
        Self {
            id: SourceId(id.into()),
            key: SourceId(key.into()),
            calls: AtomicUsize::new(0),
            answer,
        }
    }
}
#[async_trait]
impl Source for TestSource {
    fn id(&self) -> &SourceId {
        &self.id
    }
    fn identification_cache_id(&self) -> SourceId {
        self.key.clone()
    }
    async fn identify_summary(&self, summary: &FileSummary) -> Result<Option<FileMatch>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.answer {
            0 => Ok(Some(matched(self.id.clone(), summary))),
            1 => Ok(None),
            _ => anyhow::bail!("provider failed"),
        }
    }
    async fn search(&self, _: &SearchQuery) -> Result<SearchPage> {
        anyhow::bail!("unused")
    }
    async fn game_versions(&self) -> Result<Vec<String>> {
        anyhow::bail!("unused")
    }
    async fn get_project(&self, _: &ProjectRef) -> Result<Project> {
        anyhow::bail!("unused")
    }
    async fn resolve_project(&self, _: &str) -> Result<ProjectRef> {
        anyhow::bail!("unused")
    }
    async fn list_files(&self, _: &ProjectRef, _: &FileQuery) -> Result<FilePage> {
        anyhow::bail!("unused")
    }
    async fn get_file(&self, _: &FileRef) -> Result<ProjectFile> {
        anyhow::bail!("unused")
    }
}
fn matched(source: SourceId, summary: &FileSummary) -> FileMatch {
    FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source,
                id: ProjectId("project".into()),
            },
            id: FileId("release".into()),
        },
        filename: "additional.jar".into(),
        size: summary.size,
        hashes: vec![FileHash::Sha1(summary.sha1.clone())],
        role: MatchedFileRole::Additional,
    }
}

#[test]
fn source_namespaces_reuse_successes_preserve_additional_files_and_never_cache_negatives() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.jar");
    fs::write(&source, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let writer = &cache;
    let id = writer
        .import(
            &source,
            mcpm_backend::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let no_match = TestSource::new("none", "curseforge", 1);
    for _ in 0..2 {
        let outcome = runtime
            .block_on(writer.refresh_ident_info_with_outcome(
                id,
                &no_match,
                MetadataRefresh::MissingOrOutdated,
            ))
            .unwrap();
        assert_eq!(outcome.status, RefreshStatus::Unmatched);
        assert_eq!(
            outcome.info.status(),
            &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
        );
        assert!(writer.inspect(&id).unwrap().metadata.is_empty());
    }
    assert_eq!(no_match.calls.load(Ordering::SeqCst), 2);
    let official = TestSource::new("official", "curseforge", 0);
    let proxy = TestSource::new("proxy", "curseforge", 2);
    let outcome = runtime
        .block_on(writer.refresh_ident_info_with_outcome(
            id,
            &official,
            MetadataRefresh::MissingOrOutdated,
        ))
        .unwrap();
    assert_eq!(outcome.status, RefreshStatus::Matched);
    let first = outcome.info;
    let bytes = fs::read(first.path()).unwrap();
    assert_eq!(
        writer.mod_metadata(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    assert_eq!(first.status(), &CacheStatus::Available);
    let cached = first.get("curseforge").unwrap();
    assert_eq!(cached.role, MatchedFileRole::Additional);
    assert_eq!(cached.filename, "additional.jar");
    assert_eq!(
        cached.for_source(&proxy).unwrap().reference.project.source,
        proxy.id
    );
    assert!(
        cached
            .for_source(&TestSource::new("other", "other", 0))
            .is_none()
    );
    let outcome = runtime
        .block_on(writer.refresh_ident_info_with_outcome(
            id,
            &proxy,
            MetadataRefresh::MissingOrOutdated,
        ))
        .unwrap();
    assert_eq!(outcome.status, RefreshStatus::Reused);
    assert_eq!(proxy.calls.load(Ordering::SeqCst), 0);
    assert_eq!(official.calls.load(Ordering::SeqCst), 1);
    let outcome = runtime
        .block_on(writer.refresh_ident_info_with_outcome(id, &no_match, MetadataRefresh::Force))
        .unwrap();
    assert_eq!(outcome.status, RefreshStatus::Unmatched);
    assert_eq!(outcome.info.status(), &CacheStatus::Available);
    assert_eq!(outcome.info.matches().count(), 1);
    assert_eq!(outcome.info.get("curseforge"), first.get("curseforge"));
    assert_eq!(no_match.calls.load(Ordering::SeqCst), 3);
    assert!(
        runtime
            .block_on(writer.refresh_ident_info(id, &proxy, MetadataRefresh::Force))
            .is_err()
    );
    assert_eq!(fs::read(first.path()).unwrap(), bytes);
    // Loader corruption is independent of the source snapshot.
    fs::write(writer.mod_metadata(id).digest_path(), b"broken JSON").unwrap();
    assert_eq!(
        writer.mod_metadata(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    assert_eq!(writer.ident_info(id).status(), &CacheStatus::Available);
    fs::remove_file(writer.mod_metadata(id).data_path()).unwrap();
    assert_eq!(
        writer.ident_info(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
    );
    assert!(
        runtime
            .block_on(writer.refresh_ident_info(id, &official, MetadataRefresh::Force))
            .is_err()
    );
    assert!(writer.remove(&(id)).unwrap());
    assert!(!first.path().exists());
}

#[test]
fn publications_verify_content_and_merge_only_current_valid_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.jar");
    fs::write(&path, b"abc").unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let writer = &cache;
    let id = writer
        .import(&path, mcpm_backend::local::cache::CacheLinkPreference::Copy)
        .unwrap();
    let a = matched(SourceId("a".into()), &summary);
    writer.write_ident_info(id, &a).unwrap();
    let b = matched(SourceId("b".into()), &summary);
    writer.write_ident_info(id, &b).unwrap();
    let info = writer.ident_info(id);
    assert_eq!(info.matches().count(), 2);
    let bytes = fs::read(info.path()).unwrap();
    for invalid in [
        FileMatch {
            size: 4,
            ..a.clone()
        },
        FileMatch {
            hashes: vec![FileHash::Sha1("0".repeat(40))],
            ..a.clone()
        },
        FileMatch {
            hashes: vec![],
            ..a.clone()
        },
        FileMatch {
            hashes: vec![
                FileHash::Sha1(summary.sha1.clone()),
                FileHash::Sha1(summary.sha1.clone()),
            ],
            ..a.clone()
        },
        matched(SourceId("curseforge@secret".into()), &summary),
    ] {
        assert!(writer.write_ident_info(id, &invalid).is_err());
        assert_eq!(fs::read(info.path()).unwrap(), bytes);
    }
    // Incompatible/malformed snapshots are never exposed or carried into a replacement.
    for invalid in [b"{broken".to_vec(), {
        let mut data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        data["version"] = 99.into();
        serde_json::to_vec(&data).unwrap()
    }] {
        fs::write(info.path(), invalid).unwrap();
        let invalid = writer.ident_info(id);
        assert_eq!(
            invalid.status(),
            &CacheStatus::Unavailable(CacheUnavailableReason::InvalidData)
        );
        assert_eq!(invalid.matches().count(), 0);
        writer.write_ident_info(id, &a).unwrap();
        assert_eq!(writer.ident_info(id).matches().count(), 1);
    }
    fs::remove_file(writer.mod_metadata(id).data_path()).unwrap();
    fs::write(writer.mod_metadata(id).data_path(), b"xyz").unwrap();
    assert!(writer.write_ident_info(id, &a).is_err());
    fs::remove_file(writer.mod_metadata(id).data_path()).unwrap();
    assert!(writer.write_ident_info(id, &a).is_err());
}
