//! Modrinth v2 source for searching Minecraft mods and retrieving project details.
//!
//! Network operations require a Tokio runtime with I/O and time enabled.

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use reqwest::{Client, header::USER_AGENT};
use url::Url;

use super::{
    Project, ProjectDescription, ProjectId, ProjectInfo, ProjectLinks, ProjectRef, SearchPage,
    SearchQuery, SearchSort, Source, SourceId,
};

mod api;
mod catalog;
mod files;
mod identify;

/// A configured Modrinth v2 API endpoint and its source identity.
#[derive(Clone, Debug)]
pub struct ModrinthSource {
    id: SourceId,
    client: Client,
    base_url: Url,
}

impl ModrinthSource {
    /// Uses the public Modrinth v2 API without authentication.
    pub fn new(id: SourceId) -> Result<Self> {
        Self::with_endpoint(
            id,
            Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            Url::parse("https://api.modrinth.com/v2/")?,
        )
    }

    /// Uses a compatible API root and a caller-configured HTTP client.
    /// Distinct configurations should receive distinct source identifiers.
    pub fn with_endpoint(id: SourceId, client: Client, mut base_url: Url) -> Result<Self> {
        ensure!(
            matches!(base_url.scheme(), "http" | "https") && base_url.host_str().is_some(),
            "Modrinth API root must be an HTTP(S) URL"
        );
        ensure!(
            base_url.query().is_none() && base_url.fragment().is_none(),
            "Modrinth API root cannot contain a query or fragment"
        );
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        Ok(Self {
            id,
            client,
            base_url,
        })
    }

    fn request(&self, path: &str) -> Result<reqwest::RequestBuilder> {
        Ok(self.client.get(self.base_url.join(path)?).header(
            USER_AGENT,
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
        ))
    }

    fn project_info(&self, project: api::SearchHit) -> Result<ProjectInfo> {
        ensure!(
            project.project_type == "mod",
            "Modrinth project is not a mod"
        );
        // Use the opaque ID so website links do not depend on slug escaping.
        let mut website_url = Url::parse("https://modrinth.com/mod/")?;
        website_url
            .path_segments_mut()
            .expect("website URL is hierarchical")
            .pop_if_empty()
            .push(&project.project_id);
        Ok(ProjectInfo {
            reference: ProjectRef {
                source: self.id.clone(),
                id: ProjectId(project.project_id),
            },
            name: project.title,
            summary: project.description,
            slug: Some(project.slug),
            loaders: project
                .categories
                .iter()
                .filter(|tag| is_loader(tag))
                .cloned()
                .collect(),
            categories: project.categories,
            game_versions: project.versions,
            icon_url: project.icon_url,
            website_url: Some(website_url),
        })
    }
}

#[async_trait]
impl Source for ModrinthSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    async fn game_versions(&self) -> Result<Vec<String>> {
        self.fetch_game_versions().await
    }

    async fn identify_summary(
        &self,
        summary: &super::FileSummary,
    ) -> Result<Option<super::FileMatch>> {
        self.identify_summary_file(summary).await
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        ensure!(
            (1..=100).contains(&query.limit),
            "Modrinth search limit must be between 1 and 100"
        );
        ensure!(
            query.offset <= u64::from(u32::MAX),
            "Modrinth search offset exceeds its supported range"
        );
        let mut facets = vec![vec!["project_type:mod".to_owned()]];
        if !query.game_versions.is_empty() {
            facets.push(
                query
                    .game_versions
                    .iter()
                    .map(|version| format!("versions:{version}"))
                    .collect(),
            );
        }
        for loader in &query.loaders {
            check_loader(loader)?;
        }
        if !query.loaders.is_empty() {
            facets.push(
                query
                    .loaders
                    .iter()
                    .map(|loader| format!("categories:{loader}"))
                    .collect(),
            );
        }
        let sort = match query.sort {
            SearchSort::SourceDefault => "relevance",
            SearchSort::Downloads => "downloads",
            SearchSort::Updated => "updated",
            SearchSort::Newest => "newest",
        };
        let params = [
            ("query", query.query.clone()),
            ("facets", serde_json::to_string(&facets)?),
            ("index", sort.to_owned()),
            ("offset", query.offset.to_string()),
            ("limit", query.limit.to_string()),
        ];
        let response: api::SearchResponse = self
            .request("search")?
            .query(&params)
            .send()
            .await
            .context("Modrinth search request failed")?
            .error_for_status()
            .context("Modrinth search HTTP error")?
            .json()
            .await
            .context("invalid Modrinth search response")?;
        Ok(SearchPage {
            projects: response
                .hits
                .into_iter()
                .map(|p| self.project_info(p))
                .collect::<Result<_>>()?,
            offset: response.offset,
            total: Some(response.total_hits),
        })
    }

    async fn list_files(
        &self,
        project: &ProjectRef,
        query: &super::FileQuery,
    ) -> Result<super::FilePage> {
        self.list_project_files(project, query).await
    }

    async fn get_file(&self, file: &super::FileRef) -> Result<super::ProjectFile> {
        self.fetch_project_file(file).await
    }

    async fn resolve_project(&self, id_or_slug: &str) -> Result<ProjectRef> {
        ensure!(
            !id_or_slug.is_empty()
                && id_or_slug
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "invalid Modrinth project ID or slug"
        );
        let response: api::Project = self
            .request(&format!("project/{id_or_slug}"))?
            .send()
            .await
            .context("Modrinth project resolution failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid Modrinth project response")?;
        ensure!(
            response.id == id_or_slug || response.slug == id_or_slug,
            "Modrinth returned a different project"
        );
        ensure!(
            response.project_type == "mod",
            "Modrinth project is not a mod"
        );
        Ok(ProjectRef {
            source: self.id.clone(),
            id: ProjectId(response.id),
        })
    }

    async fn get_project(&self, project: &ProjectRef) -> Result<Project> {
        ensure!(
            project.source == self.id,
            "project reference does not belong to this Modrinth source"
        );
        // IDs, unlike slugs, consist solely of ASCII alphanumeric characters.
        ensure!(
            !project.id.0.is_empty() && project.id.0.bytes().all(|c| c.is_ascii_alphanumeric()),
            "invalid Modrinth project ID"
        );
        let response: api::Project = self
            .request(&format!("project/{}", project.id.0))?
            .send()
            .await
            .context("Modrinth project request failed")?
            .error_for_status()
            .context("Modrinth project HTTP error")?
            .json()
            .await
            .context("invalid Modrinth project response")?;
        ensure!(
            response.id == project.id.0,
            "Modrinth returned a different project"
        );
        let mut info = self.project_info(api::SearchHit {
            project_id: response.id,
            project_type: response.project_type,
            slug: response.slug,
            title: response.title,
            description: response.description,
            icon_url: response.icon_url,
            categories: response.categories,
            versions: response.game_versions,
        })?;
        info.loaders = response.loaders;
        Ok(Project {
            info,
            description: Some(ProjectDescription::Markdown(response.body)),
            links: ProjectLinks {
                source_code: response.source_url,
                issues: response.issues_url,
                wiki: response.wiki_url,
                discord: response.discord_url,
            },
        })
    }
}

/// Categories include gameplay tags too; accept supported loader identifiers only.
fn check_loader(loader: &str) -> Result<()> {
    ensure!(is_loader(loader), "unsupported Modrinth loader: {loader}");
    Ok(())
}

fn is_loader(loader: &str) -> bool {
    matches!(
        loader,
        "forge"
            | "fabric"
            | "quilt"
            | "neoforge"
            | "liteloader"
            | "rift"
            | "babric"
            | "bta-babric"
            | "java-agent"
            | "legacy-fabric"
            | "modloader"
            | "nilloader"
            | "ornithe"
    )
}
