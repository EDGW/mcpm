//! Single decoder and validation owner for effective palette entries from loading and editing.

use anyhow::{Result, ensure};
use serde::Deserialize;
use toml::Value;

use super::Color;

/// Effective palette data; source TOML is retained separately only for persistence.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Palette {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) types: Vec<String>,
    pub(crate) primary: Color,
    pub(crate) secondary: Color,
}

impl Palette {
    pub(super) fn decode(value: Value) -> Result<Self> {
        #[derive(Deserialize)]
        struct Fields {
            version: u32,
            id: String,
            #[serde(default = "unnamed")]
            name: String,
            types: Vec<String>,
            primary: Color,
            secondary: Color,
        }

        let fields: Fields = value.try_into()?;
        ensure!(
            fields.version == 1,
            "unsupported style version {}",
            fields.version
        );
        ensure!(
            !fields.id.is_empty()
                && fields
                    .id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')),
            "ID must contain only letters, digits, hyphens or underscores"
        );
        ensure!(
            !fields.name.trim().is_empty() && !fields.name.chars().any(char::is_control),
            "enter a valid color scheme name"
        );
        ensure!(
            !fields.types.is_empty()
                && fields
                    .types
                    .iter()
                    .all(|kind| matches!(kind.as_str(), "console" | "tui")),
            "Types must contain TUI, Console, or both"
        );
        let palette = Self {
            id: fields.id,
            name: fields.name,
            types: fields.types,
            primary: fields.primary,
            secondary: fields.secondary,
        };
        for (_, fallback, kind) in super::default_selections() {
            ensure!(
                palette.id != fallback || palette.supports(kind),
                "default palette must support {kind}"
            );
        }
        Ok(palette)
    }

    pub(super) fn supports(&self, kind: &str) -> bool {
        self.types.iter().any(|value| value == kind)
    }
}

fn unnamed() -> String {
    "Unnamed Color Scheme".into()
}
