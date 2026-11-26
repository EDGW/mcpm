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
        // pagination unless channel or version/loader alternatives need a matching offset.
        let mut params = Vec::new();
        if let [version] = query.game_versions.as_slice() {
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
        let multiple_versions = query.game_versions.len() > 1;
        let filtered = query.release_type.is_some() || multiple_loaders || multiple_versions;
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
                let channel = release_type(version.release_type);
                if query.release_type.is_some_and(|r| r != channel) {
                    continue;
                }
                if multiple_versions
                    && !version.sortable_game_versions.iter().any(|tag| {
                        tag.game_version_type_id != Some(68441)
                            && query.game_versions.contains(&tag.game_version)
                    })
                {
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
                    files.push(self.normalize_file(project, version, channel)?);
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
        let channel = release_type(response.data.release_type);
        self.normalize_file(&file.project, response.data, channel)
    }

    fn normalize_file(
        &self,
        project: &ProjectRef,
        file: api::File,
        release_type: ReleaseType,
    ) -> Result<ProjectFile> {
        ensure!(
            file.mod_id.to_string() == project.id.0,
            "CurseForge file belongs to a different project"
        );
        let mut hashes = Vec::new();
        for hash in file.hashes {
            match hash.algo {
                1 => hashes.push(FileHash::Sha1(hash.value)),
                2 => hashes.push(FileHash::Md5(hash.value)),
                other => {
                    tracing::debug!(algo = other, "Skipping unknown CurseForge hash algorithm");
                }
            }
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
        let mut dependencies = Vec::new();
        for dependency in file.dependencies {
            let kind = match dependency.relation_type {
                1 => DependencyKind::Embedded,
                2 => DependencyKind::Optional,
                3 => DependencyKind::Required,
                4 => DependencyKind::Tool,
                5 => DependencyKind::Incompatible,
                6 => DependencyKind::Include,
                other => {
                    tracing::debug!(
                        relation_type = other,
                        mod_id = dependency.mod_id,
                        "Skipping unknown CurseForge dependency relation"
                    );
                    continue;
                }
            };
            dependencies.push(FileDependency {
                target: DependencyTarget::Project(ProjectRef {
                    source: self.id.clone(),
                    id: ProjectId(dependency.mod_id.to_string()),
                }),
                kind,
            });
        }
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

fn release_type(value: u32) -> ReleaseType {
    match value {
        1 => ReleaseType::Release,
        2 => ReleaseType::Beta,
        3 => ReleaseType::Alpha,
        other => {
            tracing::debug!(release_type = other, "Unknown CurseForge release type");
            ReleaseType::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceId;

    #[test]
    fn normalize_file_tolerates_unknown_release_type_hash_algo_and_relation_type() {
        let source = CurseForgeSource::with_endpoint(
            SourceId("cf".into()),
            reqwest::Client::new(),
            "https://api.curseforge.com/v1/".parse().unwrap(),
            None,
        )
        .unwrap();
        let project = ProjectRef {
            source: SourceId("cf".into()),
            id: ProjectId("42".into()),
        };
        let raw = api::File {
            id: 100,
            mod_id: 42,
            display_name: "Demo".into(),
            file_name: "demo.jar".into(),
            release_type: 99,
            file_length: 123,
            download_url: None,
            hashes: vec![
                api::Hash {
                    algo: 1,
                    value: "abc".into(),
                },
                api::Hash {
                    algo: 99,
                    value: "future".into(),
                },
            ],
            sortable_game_versions: vec![],
            dependencies: vec![
                api::Dependency {
                    mod_id: 7,
                    relation_type: 3,
                },
                api::Dependency {
                    mod_id: 8,
                    relation_type: 99,
                },
            ],
        };
        let channel = release_type(raw.release_type);
        let file = source.normalize_file(&project, raw, channel).unwrap();
        assert_eq!(file.release_type, ReleaseType::Unknown);
        assert_eq!(file.hashes, vec![FileHash::Sha1("abc".into())]);
        assert_eq!(file.dependencies.len(), 1);
        assert_eq!(file.dependencies[0].kind, DependencyKind::Required);
    }
}
