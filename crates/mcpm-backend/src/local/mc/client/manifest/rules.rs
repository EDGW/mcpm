//! Platform and feature rule matching.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryRule {
    pub action: LibraryRuleAction,
    pub os: Option<LibraryRuleOs>,
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryRuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryRuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
