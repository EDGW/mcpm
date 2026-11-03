//! Private response models for Modrinth v2 project and search endpoints.

use serde::Deserialize;
use url::Url;

#[derive(Deserialize, Debug)]
pub(super) struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub offset: u64,
    pub total_hits: u64,
}

#[derive(Deserialize, Debug)]
pub(super) struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<Url>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub(super) struct Project {
    pub id: String,
    pub project_type: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub icon_url: Option<Url>,
    pub source_url: Option<Url>,
    pub issues_url: Option<Url>,
    pub wiki_url: Option<Url>,
    pub discord_url: Option<Url>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub(super) struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub version_type: String,
    pub date_published: chrono::DateTime<chrono::Utc>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<File>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Deserialize, Debug)]
pub(super) struct File {
    pub filename: String,
    pub size: u64,
    pub url: Url,
    pub primary: bool,
    pub hashes: Hashes,
}

#[derive(Deserialize, Debug)]
pub(super) struct Hashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Deserialize, Debug)]
pub(super) struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub file_name: Option<String>,
    pub dependency_type: String,
}
