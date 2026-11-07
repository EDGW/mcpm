//! Host publication must reject disappeared/replaced content without losing prior positive matches.

use mcpm_lib::{
    local::cache::{CacheLinkPreference, mods::ModsCacheDir},
    source::{
        FileHash, FileId, FileMatch, FileRef, FileSummary, MatchedFileRole, ProjectId, ProjectRef,
        SourceId,
    },
};

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

#[test]
fn publication_rejects_replaced_or_deleted_content_without_overwriting_identification() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.jar");
    std::fs::write(&path, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = cache.import(&path, CacheLinkPreference::Copy).unwrap();
    let expected = FileSummary::read(&path, Default::default()).unwrap();
    let publish = |file| {
        super::super::identification::publish(&cache, &id.to_string(), &expected, vec![file])
    };
    publish(matched(&expected, "modrinth")).unwrap();
    let metadata = cache.ident_info(id).path().to_owned();
    let saved = std::fs::read(&metadata).unwrap();
    let content = cache.content_path(&id);
    std::fs::remove_file(&content).unwrap();
    std::fs::write(&content, b"different bytes").unwrap();
    assert!(publish(matched(&expected, "curseforge")).is_err());
    assert_eq!(std::fs::read(&metadata).unwrap(), saved);
    std::fs::remove_file(&content).unwrap();
    assert!(publish(matched(&expected, "curseforge")).is_err());
    assert_eq!(std::fs::read(&metadata).unwrap(), saved);
}
