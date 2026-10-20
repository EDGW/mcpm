//! CurseForge file retrieval, incremental filtering, and publication normalization.

use anyhow::{Context, Result, ensure};

use super::{CurseForgeSource, api, optional_url};
use crate::source::{
    DependencyKind, DependencyTarget, FileDependency, FileHash, FileId, FilePage, FileQuery,
    FileRef, ProjectFile, ProjectId, ProjectRef, ReleaseType,
};

impl CurseForgeSource {
    fn file_project_id(&self, project: &ProjectRef) -> Result<u32> {
        ensure!(
            project.source == self.id,
            "project reference does not belong to this CurseForge source"
        );
        let id: u32 = project
            .id
            .0
            .parse()
            .context("invalid CurseForge project ID")?;
        ensure!(id > 0, "CurseForge project ID must be positive");
        Ok(id)
    }

    pub(super) async fn list_project_files(
        &self,
        project: &ProjectRef,
        query: &FileQuery,
    ) -> Result<FilePage> {
        let id = self.file_project_id(project)?;
        ensure!(query.limit > 0, "file limit must be positive");
        // The files endpoint returns newest publications first. Use native
        // pagination unless channel or multiple-loader filtering needs a matching offset.
        let mut params = Vec::new();
        if let Some(version) = &query.game_version {
            params.push(("gameVersion", version.clone()));
        }
        for loader in &query.loaders {
            let kind = match loader.as_str() {
                "forge" => 1,
                "liteloader" => 3,
                "fabric" => 4,
                "quilt" => 5,
                "neoforge" => 6,
                _ => anyhow::bail!("unsupported CurseForge loader: {loader}"),
            };
            if query.loaders.len() == 1 {
                params.push(("modLoaderType", kind.to_string()));
            }
        }
        let multiple_loaders = query.loaders.len() > 1;
        let filtered = query.release_type.is_some() || multiple_loaders;
        let mut index = if filtered { 0 } else { query.offset };
        let mut matched = 0u64;
        let mut files = Vec::new();
        loop {
            let remaining = u64::from(query.limit) - files.len() as u64;
            let page_size = if filtered { 50 } else { remaining.min(50) };
            let response: api::FileResponse = self
                .request(&format!("mods/{id}/files"))?
                .query(&params)
                .query(&[("index", index), ("pageSize", page_size)])
                .send()
                .await
                .context("CurseForge files request failed")?
                .error_for_status()?
                .json()
                .await
                .context("invalid CurseForge files response")?;
            ensure!(
                response.pagination.index == index,
                "CurseForge returned an unexpected files offset"
            );
            let count = response.data.len() as u64;
            let exhausted = index.saturating_add(count) >= response.pagination.total_count;
            for version in response.data {
                let channel = match version.release_type {
                    1 => ReleaseType::Release,
                    2 => ReleaseType::Beta,
                    3 => ReleaseType::Alpha,
                    other => anyhow::bail!("unknown CurseForge release type: {other}"),
                };
                if query.release_type.is_some_and(|r| r != channel) {
                    continue;
                }
                if multiple_loaders
                    && !version.sortable_game_versions.iter().any(|tag| {
                        tag.game_version_type_id == Some(68441)
                            && query
                                .loaders
                                .iter()
                                .any(|loader| tag.game_version_name.eq_ignore_ascii_case(loader))
                    })
                {
                    continue;
                }
                let include = !filtered || matched >= query.offset;
                matched += 1;
                if include && files.len() < query.limit as usize {
                    files.push(self.normalize_file(project, version)?);
                }
            }
            let total = if !filtered {
                Some(response.pagination.total_count)
            } else if exhausted {
                Some(matched)
            } else {
                None
            };
            if exhausted || files.len() >= query.limit as usize {
                return Ok(FilePage {
                    files,
                    offset: query.offset,
                    total,
                });
            }
            ensure!(
                count > 0,
                "CurseForge files pagination stopped before the reported total"
            );
            index = index
                .checked_add(count)
                .context("CurseForge files offset overflow")?;
        }
    }

    pub(super) async fn fetch_project_file(&self, file: &FileRef) -> Result<ProjectFile> {
        let project_id = self.file_project_id(&file.project)?;
        let id: u32 = file.id.0.parse().context("invalid CurseForge file ID")?;
        ensure!(id > 0, "CurseForge file ID must be positive");
        let response: api::Response<api::File> = self
            .request(&format!("mods/{project_id}/files/{id}"))?
            .send()
            .await
            .context("CurseForge file request failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid CurseForge file response")?;
        ensure!(
            response.data.id == id,
            "CurseForge returned a different file"
        );
        self.normalize_file(&file.project, response.data)
    }

    fn normalize_file(&self, project: &ProjectRef, file: api::File) -> Result<ProjectFile> {
        ensure!(
            file.mod_id.to_string() == project.id.0,
            "CurseForge file belongs to a different project"
        );
        let release_type = match file.release_type {
            1 => ReleaseType::Release,
            2 => ReleaseType::Beta,
            3 => ReleaseType::Alpha,
            other => anyhow::bail!("unknown CurseForge release type: {other}"),
        };
        let mut hashes = Vec::new();
        for hash in file.hashes {
            hashes.push(match hash.algo {
                1 => FileHash::Sha1(hash.value),
                2 => FileHash::Md5(hash.value),
                other => anyhow::bail!("unknown CurseForge hash algorithm: {other}"),
            });
        }
        let mut game_versions = Vec::new();
        let mut loaders = Vec::new();
        for tag in file.sortable_game_versions {
            if tag.game_version_type_id == Some(68441) {
                loaders.push(tag.game_version_name.to_ascii_lowercase());
            } else if !tag.game_version.is_empty() {
                game_versions.push(tag.game_version);
            }
        }
        game_versions.sort();
        game_versions.dedup();
        loaders.sort();
        loaders.dedup();
        let dependencies = file
            .dependencies
            .into_iter()
            .map(|dependency| {
                let kind = match dependency.relation_type {
                    1 => DependencyKind::Embedded,
                    2 => DependencyKind::Optional,
                    3 => DependencyKind::Required,
                    4 => DependencyKind::Tool,
                    5 => DependencyKind::Incompatible,
                    6 => DependencyKind::Include,
                    other => anyhow::bail!("unknown CurseForge dependency kind: {other}"),
                };
                Ok(FileDependency {
                    target: DependencyTarget::Project(ProjectRef {
                        source: self.id.clone(),
                        id: ProjectId(dependency.mod_id.to_string()),
                    }),
                    kind,
                })
            })
            .collect::<Result<_>>()?;
        Ok(ProjectFile {
            reference: FileRef {
                project: project.clone(),
                id: FileId(file.id.to_string()),
            },
            name: file.display_name,
            version: None,
            release_type,
            game_versions,
            loaders,
            filename: file.file_name,
            size: file.file_length,
            download_url: optional_url(file.download_url)?,
            hashes,
            dependencies,
        })
    }
}
