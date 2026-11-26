//! Modrinth Version normalization, primary-file selection, and dependency resolution.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};

use super::{ModrinthSource, api};
use crate::source::{
    DependencyKind, DependencyTarget, FileDependency, FileHash, FileId, FilePage, FileQuery,
    FileRef, ProjectFile, ProjectId, ProjectRef, ReleaseType,
};

impl ModrinthSource {
    fn check_project(&self, project: &ProjectRef) -> Result<()> {
        ensure!(
            project.source == self.id,
            "project reference does not belong to this Modrinth source"
        );
        check_id(&project.id.0)
    }

    async fn version(&self, id: &str) -> Result<api::Version> {
        check_id(id)?;
        let version: api::Version = self
            .request_segments(&["version", id])?
            .send()
            .await
            .context("Modrinth version request failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid Modrinth version response")?;
        ensure!(version.id == id, "Modrinth returned a different version");
        Ok(version)
    }

    pub(super) async fn list_project_files(
        &self,
        project: &ProjectRef,
        query: &FileQuery,
    ) -> Result<FilePage> {
        self.check_project(project)?;
        ensure!(query.limit > 0, "file limit must be positive");
        ensure!(
            query.loaders.iter().all(|loader| !loader.trim().is_empty()),
            "Modrinth loader filter cannot be empty"
        );
        let mut versions: Vec<api::Version> = self
            .request_segments(&["project", &project.id.0, "version"])?
            .send()
            .await
            .context("Modrinth versions request failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid Modrinth versions response")?;
        let mut matching = Vec::new();
        for version in versions.drain(..) {
            ensure!(
                version.project_id == project.id.0,
                "Modrinth version belongs to a different project"
            );
            let kind = release_type(&version.version_type);
            if (!query.game_versions.is_empty()
                && !query
                    .game_versions
                    .iter()
                    .any(|v| version.game_versions.contains(v)))
                || (!query.loaders.is_empty()
                    && !query
                        .loaders
                        .iter()
                        .any(|loader| version.loaders.contains(loader)))
                || query.release_type.is_some_and(|r| r != kind)
            {
                continue;
            }
            matching.push((version, kind));
        }
        matching.sort_by(|a, b| {
            b.0.date_published
                .cmp(&a.0.date_published)
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        let total = matching.len() as u64;
        let mut files = Vec::new();
        let mut resolved = HashMap::new();
        for (version, kind) in matching
            .into_iter()
            .skip(usize::try_from(query.offset).unwrap_or(usize::MAX))
            .take(query.limit as usize)
        {
            files.push(
                self.normalize_file(project, version, kind, &mut resolved)
                    .await?,
            );
        }
        Ok(FilePage {
            files,
            offset: query.offset,
            total: Some(total),
        })
    }

    pub(super) async fn fetch_project_file(&self, file: &FileRef) -> Result<ProjectFile> {
        self.check_project(&file.project)?;
        let version = self.version(&file.id.0).await?;
        let kind = release_type(&version.version_type);
        self.normalize_file(&file.project, version, kind, &mut HashMap::new())
            .await
    }

    async fn normalize_file(
        &self,
        project: &ProjectRef,
        version: api::Version,
        release_type: ReleaseType,
        resolved: &mut HashMap<String, String>,
    ) -> Result<ProjectFile> {
        ensure!(
            version.project_id == project.id.0,
            "Modrinth version belongs to a different project"
        );
        let mut primary = version.files.into_iter().filter(|f| f.primary);
        let file = primary
            .next()
            .context("Modrinth version has no primary file")?;
        ensure!(
            primary.next().is_none(),
            "Modrinth version has multiple primary files"
        );
        let mut hashes = Vec::new();
        if let Some(value) = file.hashes.sha1 {
            hashes.push(FileHash::Sha1(value));
        }
        if let Some(value) = file.hashes.sha512 {
            hashes.push(FileHash::Sha512(value));
        }
        let mut dependencies = Vec::new();
        for dependency in version.dependencies {
            let kind = match dependency.dependency_type.as_str() {
                "required" => DependencyKind::Required,
                "optional" => DependencyKind::Optional,
                "incompatible" => DependencyKind::Incompatible,
                "embedded" => DependencyKind::Embedded,
                other => {
                    tracing::debug!(
                        dependency_type = other,
                        project_id = dependency.project_id.as_deref(),
                        version_id = dependency.version_id.as_deref(),
                        "Skipping unknown Modrinth dependency kind"
                    );
                    continue;
                }
            };
            let target = if let Some(id) = dependency.version_id {
                let owner = if let Some(owner) = dependency.project_id {
                    owner
                } else if let Some(owner) = resolved.get(&id) {
                    owner.clone()
                } else {
                    let owner = self.version(&id).await?.project_id;
                    resolved.insert(id.clone(), owner.clone());
                    owner
                };
                DependencyTarget::File(FileRef {
                    project: ProjectRef {
                        source: self.id.clone(),
                        id: ProjectId(owner),
                    },
                    id: FileId(id),
                })
            } else if let Some(id) = dependency.project_id {
                DependencyTarget::Project(ProjectRef {
                    source: self.id.clone(),
                    id: ProjectId(id),
                })
            } else {
                DependencyTarget::Filename(
                    dependency
                        .file_name
                        .filter(|s| !s.is_empty())
                        .context("Modrinth dependency has no target")?,
                )
            };
            dependencies.push(FileDependency { target, kind });
        }
        Ok(ProjectFile {
            reference: FileRef {
                project: project.clone(),
                id: FileId(version.id),
            },
            name: version.name,
            version: Some(version.version_number),
            release_type,
            game_versions: version.game_versions,
            loaders: version.loaders,
            filename: file.filename,
            size: file.size,
            download_url: Some(file.url),
            hashes,
            dependencies,
        })
    }
}

fn check_id(id: &str) -> Result<()> {
    super::check_path_segment(id, "Modrinth ID")
}

fn release_type(value: &str) -> ReleaseType {
    match value {
        "release" => ReleaseType::Release,
        "beta" => ReleaseType::Beta,
        "alpha" => ReleaseType::Alpha,
        other => {
            tracing::debug!(version_type = other, "Unknown Modrinth release type");
            ReleaseType::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceId;

    #[test]
    fn check_id_enforces_url_path_segment_safety_without_alphanumeric_assumptions() {
        for valid in ["abc123", "proj-1_v2.0", "opaque:id+1"] {
            assert!(check_id(valid).is_ok(), "{valid:?}");
        }
        for invalid in [
            "",
            ".",
            "..",
            "a b",
            "a\tb",
            "a\nb",
            "a\0b",
            "a/b",
            "a\\b",
            "a?b",
            "a#b",
        ] {
            assert!(check_id(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn normalize_file_tolerates_unknown_version_type_and_dependency_type() {
        let source = ModrinthSource::with_endpoint(
            SourceId("mr".into()),
            reqwest::Client::new(),
            "https://api.modrinth.com/v2/".parse().unwrap(),
        )
        .unwrap();
        let project = ProjectRef {
            source: SourceId("mr".into()),
            id: ProjectId("proj-1".into()),
        };
        let version = api::Version {
            id: "ver-1".into(),
            project_id: "proj-1".into(),
            name: "Demo".into(),
            version_number: "1.0.0".into(),
            version_type: "snapshot".into(),
            date_published: chrono::DateTime::UNIX_EPOCH,
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            files: vec![api::File {
                filename: "demo.jar".into(),
                size: 42,
                url: "https://cdn.modrinth.com/demo.jar".parse().unwrap(),
                primary: true,
                hashes: api::Hashes {
                    sha1: None,
                    sha512: None,
                },
            }],
            dependencies: vec![
                api::Dependency {
                    project_id: Some("dep-known".into()),
                    version_id: None,
                    file_name: None,
                    dependency_type: "optional".into(),
                },
                api::Dependency {
                    project_id: None,
                    version_id: None,
                    file_name: None,
                    dependency_type: "future_relation".into(),
                },
            ],
        };
        let kind = release_type(&version.version_type);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let file = runtime
            .block_on(source.normalize_file(&project, version, kind, &mut HashMap::new()))
            .unwrap();
        assert_eq!(file.release_type, ReleaseType::Unknown);
        assert_eq!(file.dependencies.len(), 1);
        assert_eq!(file.dependencies[0].kind, DependencyKind::Optional);
    }
}
