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
            .request(&format!("version/{id}"))?
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
        for loader in &query.loaders {
            super::check_loader(loader)?;
        }
        let mut versions: Vec<api::Version> = self
            .request(&format!("project/{}/version", project.id.0))?
            .send()
            .await
            .context("Modrinth versions request failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid Modrinth versions response")?;
        // Parse all release kinds before filtering, so unknown data is not silently discarded.
        let mut matching = Vec::new();
        for version in versions.drain(..) {
            ensure!(
                version.project_id == project.id.0,
                "Modrinth version belongs to a different project"
            );
            let kind = release_type(&version.version_type)?;
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
            matching.push(version);
        }
        matching.sort_by(|a, b| {
            b.date_published
                .cmp(&a.date_published)
                .then_with(|| a.id.cmp(&b.id))
        });
        let total = matching.len() as u64;
        let mut files = Vec::new();
        let mut resolved = HashMap::new();
        for version in matching
            .into_iter()
            .skip(usize::try_from(query.offset).unwrap_or(usize::MAX))
            .take(query.limit as usize)
        {
            files.push(self.normalize_file(project, version, &mut resolved).await?);
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
        self.normalize_file(&file.project, version, &mut HashMap::new())
            .await
    }

    async fn normalize_file(
        &self,
        project: &ProjectRef,
        version: api::Version,
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
                other => anyhow::bail!("unknown Modrinth dependency kind: {other}"),
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
            release_type: release_type(&version.version_type)?,
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
    ensure!(
        !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric()),
        "invalid Modrinth ID"
    );
    Ok(())
}

fn release_type(value: &str) -> Result<ReleaseType> {
    match value {
        "release" => Ok(ReleaseType::Release),
        "beta" => Ok(ReleaseType::Beta),
        "alpha" => Ok(ReleaseType::Alpha),
        other => anyhow::bail!("unknown Modrinth release type: {other}"),
    }
}
