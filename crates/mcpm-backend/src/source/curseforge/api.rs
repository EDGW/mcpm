//! Private response models for the CurseForge v1 API.

use serde::Deserialize;
use url::Url;

#[derive(Deserialize, Debug)]
pub(super) struct Response<T> {
    pub data: T,
}

#[derive(Deserialize, Debug)]
pub(super) struct SearchResponse {
    pub data: Vec<Project>,
    pub pagination: Pagination,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pagination {
    pub index: u64,
    pub total_count: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Project {
    pub id: u32,
    pub game_id: u32,
    pub class_id: Option<u32>,
    pub name: String,
    pub summary: String,
    pub slug: String,
    pub logo: Option<Logo>,
    pub links: Links,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub latest_files_indexes: Vec<FileIndex>,
    #[serde(default)]
    pub latest_early_access_files_indexes: Vec<FileIndex>,
    #[serde(default)]
    pub latest_files: Vec<RecentFile>,
}

#[derive(Deserialize, Debug)]
pub(super) struct Category {
    pub slug: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileIndex {
    pub game_version: String,
    pub mod_loader: Option<u32>,
}

/// Only the compatibility summary is needed from files embedded in a project.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecentFile {
    #[serde(default)]
    pub sortable_game_versions: Vec<GameVersion>,
}

#[derive(Deserialize, Debug)]
pub(super) struct Logo {
    pub url: Url,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Links {
    pub website_url: Option<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
}

#[derive(Deserialize, Debug)]
pub(super) struct FileResponse {
    pub data: Vec<File>,
    pub pagination: Pagination,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct File {
    pub id: u32,
    pub mod_id: u32,
    pub display_name: String,
    pub file_name: String,
    pub release_type: u32,
    pub file_length: u64,
    pub download_url: Option<String>,
    pub hashes: Vec<Hash>,
    pub sortable_game_versions: Vec<GameVersion>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct GameVersion {
    pub game_version_name: String,
    pub game_version_type_id: Option<u32>,
    pub game_version: String,
}

#[derive(Deserialize, Debug)]
pub(super) struct Hash {
    pub algo: u32,
    pub value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(super) struct Dependency {
    pub mod_id: u32,
    pub relation_type: u32,
}
