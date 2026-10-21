//! CurseForge v1 source, with configurable endpoints for compatible proxies.
//!
//! Network operations require a Tokio runtime with I/O and time enabled.

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use reqwest::{
    Client,
    header::{HeaderValue, USER_AGENT},
};
use url::Url;

use super::{
    Project, ProjectDescription, ProjectId, ProjectInfo, ProjectLinks, ProjectRef, SearchPage,
    SearchQuery, SearchSort, Source, SourceId,
};

mod api;
mod files;
mod identify;

/// A configured CurseForge API endpoint and its source identity.
#[derive(Clone)]
pub struct CurseForgeSource {
    id: SourceId,
    client: Client,
    base_url: Url,
    api_key: Option<HeaderValue>,
}

impl CurseForgeSource {
    /// Uses the official API with the supplied key.
    pub fn new(id: SourceId, api_key: &str) -> Result<Self> {
        Self::with_endpoint(
            id,
            Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            Url::parse("https://api.curseforge.com/v1/")?,
            Some(api_key),
        )
    }

    /// Uses a compatible API root, e.g. `https://api.curse.tools/v1/cf/`.
    /// Pass `None` for endpoints that do not require a key.
    /// Distinct configurations should receive distinct source identifiers.
    pub fn with_endpoint(
        id: SourceId,
        client: Client,
        mut base_url: Url,
        api_key: Option<&str>,
    ) -> Result<Self> {
        ensure!(
            matches!(base_url.scheme(), "http" | "https") && base_url.host_str().is_some(),
            "CurseForge API root must be an HTTP(S) URL"
        );
        ensure!(
            base_url.query().is_none() && base_url.fragment().is_none(),
            "CurseForge API root cannot contain a query or fragment"
        );
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        // Curse.tools redirects the /v1/cf/ alias with HTTP 302, which turns
        // POST fingerprint requests into GET. Use its canonical API root.
        if base_url.host_str() == Some("api.curse.tools") && base_url.path() == "/v1/cf/" {
            base_url.set_path("/v1/");
        }
        let api_key = api_key
            .map(|key| {
                let mut value =
                    HeaderValue::from_str(key).context("invalid CurseForge API key header")?;
                value.set_sensitive(true);
                Ok::<_, anyhow::Error>(value)
            })
            .transpose()?;
        Ok(Self {
            id,
            client,
            base_url,
            api_key,
        })
    }

    fn request(&self, path: &str) -> Result<reqwest::RequestBuilder> {
        self.request_with_method(reqwest::Method::GET, path)
    }

    fn request_with_method(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let mut request = self
            .client
            .request(method, self.base_url.join(path)?)
            .header(
                USER_AGENT,
                concat!("kako-mods-lib/", env!("CARGO_PKG_VERSION")),
            );
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key.clone());
        }
        Ok(request)
    }

    fn project_info(&self, project: api::Project) -> Result<ProjectInfo> {
        ensure!(
            project.game_id == 432 && project.class_id == Some(6),
            "CurseForge project is not a Minecraft mod"
        );
        Ok(ProjectInfo {
            reference: ProjectRef {
                source: self.id.clone(),
                id: ProjectId(project.id.to_string()),
            },
            name: project.name,
            summary: project.summary,
            slug: Some(project.slug),
            icon_url: project.logo.map(|logo| logo.url),
            website_url: optional_url(project.links.website_url)?,
        })
    }
}

#[async_trait]
impl Source for CurseForgeSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    async fn identify_file(&self, path: &std::path::Path) -> Result<Option<super::FileMatch>> {
        self.identify_local_file(path).await
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        ensure!(
            (1..=50).contains(&query.limit),
            "CurseForge search limit must be between 1 and 50"
        );
        ensure!(
            query.offset <= 10000 - u64::from(query.limit),
            "CurseForge search must remain within the first 10000 results"
        );
        let mut params = vec![
            ("gameId", "432".to_owned()),
            ("classId", "6".to_owned()),
            ("searchFilter", query.query.clone()),
            ("index", query.offset.to_string()),
            ("pageSize", query.limit.to_string()),
        ];
        ensure!(
            query.game_versions.len() <= 4,
            "CurseForge search supports at most 4 Minecraft versions"
        );
        ensure!(
            query.loaders.len() <= 5,
            "CurseForge search supports at most 5 loaders"
        );
        match query.game_versions.as_slice() {
            [] => {}
            [version] => params.push(("gameVersion", version.clone())),
            versions => params.push(("gameVersions", serde_json::to_string(versions)?)),
        }
        let loaders = query
            .loaders
            .iter()
            .map(|loader| match loader.as_str() {
                "forge" => Ok(1),
                "liteloader" => Ok(3),
                "fabric" => Ok(4),
                "quilt" => Ok(5),
                "neoforge" => Ok(6),
                _ => anyhow::bail!("unsupported CurseForge loader: {loader}"),
            })
            .collect::<Result<Vec<u8>>>()?;
        match loaders.as_slice() {
            [] => {}
            [loader] => params.push(("modLoaderType", loader.to_string())),
            loaders => params.push(("modLoaderTypes", serde_json::to_string(loaders)?)),
        }
        let sort = match query.sort {
            SearchSort::SourceDefault => None,
            SearchSort::Downloads => Some("6"),
            SearchSort::Updated => Some("3"),
            SearchSort::Newest => {
                anyhow::bail!("CurseForge does not support sorting projects by creation time")
            }
        };
        if let Some(sort) = sort {
            params.push(("sortField", sort.to_owned()));
            params.push(("sortOrder", "desc".to_owned()));
        }
        let response: api::SearchResponse = self
            .request("mods/search")?
            .query(&params)
            .send()
            .await
            .context("CurseForge search request failed")?
            .error_for_status()
            .context("CurseForge search HTTP error")?
            .json()
            .await
            .context("invalid CurseForge search response")?;
        Ok(SearchPage {
            projects: response
                .data
                .into_iter()
                .map(|p| self.project_info(p))
                .collect::<Result<_>>()?,
            offset: response.pagination.index,
            total: Some(response.pagination.total_count),
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
        if id_or_slug.bytes().all(|c| c.is_ascii_digit()) && !id_or_slug.is_empty() {
            let id: u32 = id_or_slug
                .parse()
                .context("invalid CurseForge project ID")?;
            ensure!(id > 0, "CurseForge project ID must be positive");
            return Ok(ProjectRef {
                source: self.id.clone(),
                id: ProjectId(id.to_string()),
            });
        }
        ensure!(
            !id_or_slug.is_empty()
                && id_or_slug
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "invalid CurseForge project slug"
        );
        let response: api::SearchResponse = self
            .request("mods/search")?
            .query(&[("gameId", "432"), ("classId", "6"), ("slug", id_or_slug)])
            .send()
            .await
            .context("CurseForge slug resolution failed")?
            .error_for_status()?
            .json()
            .await
            .context("invalid CurseForge slug response")?;
        let project = response
            .data
            .into_iter()
            .find(|p| p.slug == id_or_slug)
            .context("CurseForge project slug not found")?;
        Ok(self.project_info(project)?.reference)
    }

    async fn get_project(&self, project: &ProjectRef) -> Result<Project> {
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
        let response: api::Response<api::Project> = self
            .request(&format!("mods/{id}"))?
            .send()
            .await
            .context("CurseForge project request failed")?
            .error_for_status()
            .context("CurseForge project HTTP error")?
            .json()
            .await
            .context("invalid CurseForge project response")?;
        ensure!(
            response.data.id == id,
            "CurseForge returned a different project"
        );
        let links = ProjectLinks {
            source_code: optional_url(response.data.links.source_url.clone())?,
            issues: optional_url(response.data.links.issues_url.clone())?,
            wiki: optional_url(response.data.links.wiki_url.clone())?,
            discord: None,
        };
        let info = self.project_info(response.data)?;
        let description: api::Response<String> = self
            .request(&format!("mods/{id}/description"))?
            .send()
            .await
            .context("CurseForge description request failed")?
            .error_for_status()
            .context("CurseForge description HTTP error")?
            .json()
            .await
            .context("invalid CurseForge description response")?;
        Ok(Project {
            info,
            description: Some(ProjectDescription::Html(description.data)),
            links,
        })
    }
}

/// CurseForge represents unset links using both null and empty strings.
fn optional_url(value: Option<String>) -> Result<Option<Url>> {
    value
        .filter(|s| !s.is_empty())
        .map(|s| Url::parse(&s).context("invalid CurseForge project link"))
        .transpose()
}
