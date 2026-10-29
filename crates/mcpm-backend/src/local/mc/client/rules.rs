//! Platform and feature rule matching.

use std::collections::BTreeMap;

use regex::Regex;
use thiserror::Error;

use super::manifest::{LibraryRule, LibraryRuleAction};

/// Explicit target platform, using launcher OS names (`windows`, `linux`, `osx`)
/// and the architecture/version strings against which manifest regexes match.
#[derive(Debug, Clone)]
pub struct ClientPlatform {
    pub os: String,
    pub arch: String,
    pub version: String,
    /// Used to expand legacy native classifier `${arch}` (32 or 64).
    pub bits: u8,
}

/// Caller-supplied environment used to evaluate library rules and select natives.
#[derive(Debug, Clone)]
pub struct RuleEnvironment {
    pub platform: ClientPlatform,
    /// Missing features do not satisfy a rule, matching HMCL's rule evaluation.
    pub features: BTreeMap<String, bool>,
}

impl RuleEnvironment {
    pub fn new(platform: ClientPlatform) -> Self {
        Self {
            platform,
            features: BTreeMap::new(),
        }
    }

    /// No rules means allowed; otherwise the last matching rule wins, starting
    /// with disallow. OS architecture/version patterns match the whole string.
    pub fn allows(&self, rules: &[LibraryRule]) -> Result<bool, LibraryRuleError> {
        let mut allowed = rules.is_empty();
        for rule in rules {
            if let Some(os) = &rule.os {
                if os
                    .name
                    .as_ref()
                    .is_some_and(|name| name != &self.platform.os)
                {
                    continue;
                }
                if !matches_pattern(os.arch.as_deref(), &self.platform.arch)?
                    || !matches_pattern(os.version.as_deref(), &self.platform.version)?
                {
                    continue;
                }
            }
            if rule
                .features
                .iter()
                .any(|(name, value)| self.features.get(name) != Some(value))
            {
                continue;
            }
            allowed = rule.action == LibraryRuleAction::Allow;
        }
        Ok(allowed)
    }
}

fn matches_pattern(pattern: Option<&str>, value: &str) -> Result<bool, LibraryRuleError> {
    let Some(pattern) = pattern else {
        return Ok(true);
    };
    let regex = Regex::new(&format!("\\A(?:{pattern})\\z")).map_err(|source| LibraryRuleError {
        pattern: pattern.to_owned(),
        source,
    })?;
    Ok(regex.is_match(value))
}

#[derive(Debug, Error)]
#[error("invalid library rule pattern {pattern:?}")]
pub struct LibraryRuleError {
    pub pattern: String,
    #[source]
    pub source: regex::Error,
}
