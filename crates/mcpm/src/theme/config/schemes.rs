//! Color scheme management for independent Console/TUI dark and light categories.

use super::Config;
use anyhow::{Context, Result, ensure};
use std::path::Path;
use toml::Value;

impl Config {
    pub(crate) fn palette_types(&self, id: &str) -> &[String] {
        &self.styles[id].types
    }

    pub(crate) fn palette_secondary(&self, id: &str) -> super::Color {
        self.styles[id].secondary
    }

    pub(crate) fn palette_primary(&self, id: &str) -> super::Color {
        self.styles[id].primary
    }

    pub(crate) fn selected_palettes(&self) -> [String; 4] {
        [
            self.settings.console_style_dark.clone(),
            self.settings.console_style_light.clone(),
            self.settings.tui_style_dark.clone(),
            self.settings.tui_style_light.clone(),
        ]
    }

    pub(crate) fn palette_name(&self, id: &str) -> String {
        self.documents
            .values()
            .filter_map(|doc| doc.get("style"))
            .filter_map(Value::as_array)
            .flatten()
            .find(|style| style["id"].as_str() == Some(id))
            .and_then(|style| style.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Unnamed Color Scheme")
            .to_owned()
    }

    pub(crate) fn create_palette(&mut self, key: &str, id: &str) -> Result<String> {
        ensure!(
            !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')),
            "ID must contain only letters, digits, hyphens or underscores"
        );
        ensure!(
            !self.styles.contains_key(id),
            "color scheme ID {id:?} already exists"
        );
        let source = self
            .entries()
            .into_iter()
            .find(|(entry, _)| entry == key)
            .context("unknown color theme")?
            .1;
        let id = id.to_owned();
        let mut palette = self
            .documents
            .values()
            .filter_map(|doc| doc.get("style"))
            .filter_map(Value::as_array)
            .flatten()
            .find(|style| style["id"].as_str() == Some(&source))
            .context("source color scheme not found")?
            .clone();
        palette["id"] = Value::String(id.clone());
        palette["name"] = Value::String(id.clone());
        let mut documents = self.documents.clone();
        documents
            .entry("styles/custom.toml".into())
            .or_insert_with(|| Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .entry("style")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(palette);
        if let Some(deleted) = documents
            .get_mut("styles/custom.toml")
            .unwrap()
            .get_mut("deleted")
            .and_then(Value::as_array_mut)
        {
            deleted.retain(|value| value.as_str() != Some(&id));
        }
        let mut next = Self::parse(documents)?;
        next.set(key, &id)?;
        *self = next;
        Ok(id)
    }

    /// Move the effective palette into the panel-owned collection before editing it.
    pub(crate) fn customize_palette(&mut self, id: &str) -> Result<()> {
        let palette = self
            .documents
            .values()
            .filter_map(|doc| doc.get("style"))
            .filter_map(Value::as_array)
            .flatten()
            .find(|style| style["id"].as_str() == Some(id))
            .context("color scheme not found")?
            .clone();
        for document in self.documents.values_mut() {
            if let Some(styles) = document.get_mut("style").and_then(Value::as_array_mut) {
                styles.retain(|style| style["id"].as_str() != Some(id));
            }
        }
        self.documents
            .entry("styles/custom.toml".into())
            .or_insert_with(|| Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .entry("style")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(palette);
        Ok(())
    }

    pub(crate) fn rename_palette(&mut self, id: &str, name: &str) -> Result<()> {
        ensure!(
            !name.trim().is_empty() && !name.chars().any(char::is_control),
            "enter a valid color scheme name"
        );
        self.customize_palette(id)?;
        self.set(&format!("styles.{id}.name"), name)
    }

    pub(crate) fn delete_palette(&mut self, id: &str) -> Result<()> {
        ensure!(
            !Self::defaults().styles.contains_key(id),
            "bundled color schemes cannot be deleted"
        );
        ensure!(self.styles.contains_key(id), "color scheme not found");
        let mut next = self.clone();
        for (index, selected) in self.selected_palettes().iter().enumerate() {
            if selected == id {
                next.set(
                    [
                        "settings.console_style_dark",
                        "settings.console_style_light",
                        "settings.tui_style_dark",
                        "settings.tui_style_light",
                    ][index],
                    &super::default_selections()[index].1,
                )?;
            }
        }
        for document in next.documents.values_mut() {
            if let Some(styles) = document.get_mut("style").and_then(Value::as_array_mut) {
                styles.retain(|style| style.get("id").and_then(Value::as_str) != Some(id));
            }
        }
        // Preserve collection files while keeping deletions effective on reload.
        next.documents
            .entry("styles/custom.toml".into())
            .or_insert_with(|| Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .entry("deleted")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(Value::String(id.into()));
        *self = Self::parse(next.documents)?;
        Ok(())
    }

    pub(crate) fn save_styles(&self, directory: &Path) -> Result<()> {
        if self.documents.contains_key("styles/custom.toml") {
            self.save_document(directory, "styles/custom.toml")?;
        }
        self.save_key(directory, "settings.console_style_dark")?;
        self.save_key(directory, "loader_colors.console.fabric")
    }
}
