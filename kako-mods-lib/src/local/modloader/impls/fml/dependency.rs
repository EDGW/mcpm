//! Shared Forge/NeoForge declaration parsing with explicit format-specific rules.

use anyhow::{Context, Result};
use serde_json::Value;
use thiserror::Error;

use crate::local::mcmod::{
    Dependency, DependencyConstraint, DependencyKind, DependencyOrdering, DependencyQualifiers,
    DependencyReference, DependencySide,
};

use super::super::{constraints, metadata};

#[derive(Clone, Copy)]
pub(in super::super) enum DependencyFormat {
    Forge,
    NeoForge,
    /// Older mods.toml can contain either mandatory or type declarations.
    NeoForgeLegacy,
}

#[derive(Debug, Error)]
enum DependencyParseError {
    #[error("missing required dependency field mandatory")]
    MissingMandatory,
    #[error(
        "invalid dependency type {value:?}: expected required, optional, incompatible, or discouraged (case-insensitive)"
    )]
    InvalidType { value: String },
    #[error("invalid dependency side {value:?}: expected CLIENT, SERVER, or BOTH")]
    InvalidSide { value: String },
    #[error("invalid dependency ordering {value:?}: expected NONE, BEFORE, or AFTER")]
    InvalidOrdering { value: String },
}

pub(super) fn parse(
    clause: &Value,
    document: &str,
    format: DependencyFormat,
) -> Result<Dependency> {
    let id = metadata::required_string(clause, "modId", document)?;
    let version = metadata::optional_string(clause, "versionRange", document)?.unwrap_or("");
    Ok(Dependency {
        kind: kind(clause, document, format)?,
        constraint: DependencyConstraint::Reference(DependencyReference {
            id: id.to_owned(),
            version_requirement: constraints::maven(version)
                .with_context(|| format!("{document}: invalid dependency range for {id}"))?,
            qualifiers: qualifiers(clause, document)?,
        }),
    })
}

pub(super) fn kind(
    clause: &Value,
    document: &str,
    format: DependencyFormat,
) -> Result<DependencyKind> {
    // A type declaration takes precedence in the transitional NeoForge format.
    let forge = match format {
        DependencyFormat::Forge => true,
        DependencyFormat::NeoForge => false,
        DependencyFormat::NeoForgeLegacy => {
            clause.get("type").is_none() && clause.get("mandatory").is_some()
        }
    };
    if forge {
        let mandatory = metadata::optional_bool(clause, "mandatory", document)?
            .ok_or(DependencyParseError::MissingMandatory)?;
        return Ok(if mandatory {
            DependencyKind::Required
        } else {
            DependencyKind::Optional
        });
    }

    let value = metadata::optional_string(clause, "type", document)?.unwrap_or("required");
    // NeoForge normalizes every type value, not just REQUIRED.
    match value.to_ascii_uppercase().as_str() {
        "REQUIRED" => Ok(DependencyKind::Required),
        "OPTIONAL" => Ok(DependencyKind::Optional),
        "INCOMPATIBLE" => Ok(DependencyKind::Breaks),
        "DISCOURAGED" => Ok(DependencyKind::Conflicts),
        _ => Err(DependencyParseError::InvalidType {
            value: value.to_owned(),
        }
        .into()),
    }
}

fn qualifiers(clause: &Value, document: &str) -> Result<DependencyQualifiers> {
    // Both loaders use case-sensitive enum values for side and ordering.
    let side = match metadata::optional_string(clause, "side", document)?.unwrap_or("BOTH") {
        "CLIENT" => DependencySide::Client,
        "SERVER" => DependencySide::Server,
        "BOTH" => DependencySide::Both,
        value => {
            return Err(DependencyParseError::InvalidSide {
                value: value.to_owned(),
            }
            .into());
        }
    };
    let ordering = match metadata::optional_string(clause, "ordering", document)?.unwrap_or("NONE")
    {
        "NONE" => DependencyOrdering::None,
        "BEFORE" => DependencyOrdering::BeforeTarget,
        "AFTER" => DependencyOrdering::AfterTarget,
        value => {
            return Err(DependencyParseError::InvalidOrdering {
                value: value.to_owned(),
            }
            .into());
        }
    };
    Ok(DependencyQualifiers {
        side: Some(side),
        ordering: Some(ordering),
        reason: metadata::optional_string(clause, "reason", document)?.map(str::to_owned),
        ..Default::default()
    })
}
