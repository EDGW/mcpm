//! Cache-first identification, exact aggregate classes, and failure isolation without provider networking.

use super::*;
use crate::operations::mods::source::Configuration;
use mcpm_lib::{
    local::cache::{CacheLinkPreference, mods::ModsCacheDir},
    source::{FileHash, FileId, FileRef, MatchedFileRole, ProjectId, ProjectRef, SourceId},
};
use std::sync::Arc;

fn matched(summary: &FileSummary, source: &str) -> FileMatch {
    FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId(source.into()),
                id: ProjectId("project".into()),
            },
            id: FileId("file".into()),
        },
        filename: "mod.jar".into(),
        size: summary.size,
        hashes: vec![FileHash::Sha1(summary.sha1.clone())],
        role: MatchedFileRole::Primary,
    }
}

#[tokio::test]
async fn valid_cache_ignores_unusable_credentials_but_force_does_not() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mod.jar");
    std::fs::write(&path, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = cache.import(&path, CacheLinkPreference::Copy).unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    publish(
        &cache,
        &id.to_string(),
        &summary,
        vec![matched(&summary, "curseforge")],
    )
    .unwrap();
    let metadata = cache.ident_info(id).path().to_owned();
    let saved = std::fs::read(&metadata).unwrap();
    let sources = Sources::new(vec![Configuration::from("curseforge@")]).unwrap();
    let prepared = prepare(
        &cache.content_path(&id),
        sources.requirements(),
        sources.cache_ids(),
        Some((&cache, &id.to_string())),
        false,
    )
    .unwrap();
    let reused = sources.identify(&prepared, false).await;
    assert_eq!(reused.status(), QueryStatus::Success);
    assert_eq!(reused.cache_coverage(), CacheCoverage::All);
    assert!(matches!(
        reused.sources[0].resolution,
        SourceResolution::Cached(_)
    ));
    let forced = prepare(
        &cache.content_path(&id),
        sources.requirements(),
        sources.cache_ids(),
        Some((&cache, &id.to_string())),
        true,
    )
    .unwrap();
    let rejected = sources.identify(&forced, false).await;
    assert_eq!(rejected.status(), QueryStatus::Failed);
    assert_eq!(rejected.cache_coverage(), CacheCoverage::None);
    assert!(matches!(
        rejected.sources[0].resolution,
        SourceResolution::Failed(_)
    ));
    assert_eq!(std::fs::read(metadata).unwrap(), saved);
}

#[tokio::test]
async fn first_source_failure_does_not_erase_a_later_cache_hit_or_echo_its_secret() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mod.jar");
    std::fs::write(&path, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = cache.import(&path, CacheLinkPreference::Copy).unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    publish(
        &cache,
        &id.to_string(),
        &summary,
        vec![matched(&summary, "curseforge")],
    )
    .unwrap();
    let sources = Sources::new(vec![
        Configuration::from("invalid@DO_NOT_DISCLOSE_THIS"),
        Configuration::from("curseforge@"),
    ])
    .unwrap();
    let prepared = prepare(
        &cache.content_path(&id),
        sources.requirements(),
        sources.cache_ids(),
        Some((&cache, &id.to_string())),
        false,
    )
    .unwrap();
    let result = sources.identify(&prepared, true).await;
    assert_eq!(result.status(), QueryStatus::PartialFailed);
    assert_eq!(result.cache_coverage(), CacheCoverage::Partial);
    assert!(matches!(
        result.sources[0].resolution,
        SourceResolution::Failed(_)
    ));
    assert!(matches!(
        result.sources[1].resolution,
        SourceResolution::Cached(_)
    ));
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("DO_NOT_DISCLOSE_THIS")
    );
}

#[test]
fn five_aggregate_classes_distinguish_cache_reuse_from_fresh_partial_results() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mod.jar");
    std::fs::write(&path, b"abc").unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    let cached = || SourceResolution::Cached(matched(&summary, "one"));
    let queried = || SourceResolution::Queried(Some(matched(&summary, "one")));
    let failed = || SourceResolution::Failed(Arc::new(anyhow::anyhow!("query failed").into()));
    let cases = [
        (
            vec![cached(), cached()],
            QueryStatus::Success,
            CacheCoverage::All,
        ),
        (
            vec![cached(), SourceResolution::Queried(None)],
            QueryStatus::Success,
            CacheCoverage::Partial,
        ),
        (
            vec![cached(), failed()],
            QueryStatus::PartialFailed,
            CacheCoverage::Partial,
        ),
        (
            vec![queried(), SourceResolution::Queried(None)],
            QueryStatus::Success,
            CacheCoverage::None,
        ),
        (
            vec![queried(), failed()],
            QueryStatus::Failed,
            CacheCoverage::None,
        ),
    ];
    let mut counts = QuerySummary {
        total: cases.len(),
        ..Default::default()
    };
    for (resolutions, status, coverage) in cases {
        let result = QueryResult {
            sources: resolutions
                .into_iter()
                .enumerate()
                .map(|(index, resolution)| SourceResult {
                    source: SourceId(index.to_string()),
                    cache_id: Some(SourceId(index.to_string())),
                    resolution,
                })
                .collect(),
        };
        assert_eq!(result.status(), status);
        assert_eq!(result.cache_coverage(), coverage);
        counts.record(&result);
    }
    assert_eq!(
        (counts.success, counts.partial_failed, counts.failed),
        (3, 1, 1)
    );
    assert_eq!(counts.completed(), 5);
}

#[test]
fn invalid_provider_identity_does_not_discard_an_independent_positive_match() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mod.jar");
    std::fs::write(&path, b"abc").unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    let result = QueryResult {
        sources: vec![
            SourceResult {
                source: SourceId("first".into()),
                cache_id: Some(SourceId("first".into())),
                resolution: SourceResolution::Queried(Some(matched(&summary, "wrong-provider"))),
            },
            SourceResult {
                source: SourceId("second".into()),
                cache_id: Some(SourceId("second".into())),
                resolution: SourceResolution::Queried(Some(matched(&summary, "second"))),
            },
        ],
    };
    let mut matches = result.new_matches().into_iter();
    let (source, invalid) = matches.next().unwrap();
    assert_eq!(source.0, "first");
    assert!(invalid.is_err());
    let (source, valid) = matches.next().unwrap();
    assert_eq!(source.0, "second");
    assert_eq!(valid.unwrap().reference.project.source.0, "second");
    assert!(matches.next().is_none());
}
