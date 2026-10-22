//! Load embedded defaults and user overrides; validate and atomically save customization.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use toml::Value;

use super::Color;

mod loading;
mod schemes;

const DEFAULTS: [(&str, &str); 4] = [
    (
        "settings.toml",
        include_str!("../../../config/settings.toml"),
    ),
    (
        "loader_colors.toml",
        include_str!("../../../config/loader_colors.toml"),
    ),
    (
        "styles/tui.toml",
        include_str!("../../../config/styles/tui.toml"),
    ),
    (
        "source_colors.toml",
        include_str!("../../../config/source_colors.toml"),
    ),
];

#[derive(Clone, Debug)]
pub(crate) struct Config {
    documents: BTreeMap<String, Value>,
    settings: Settings,
    styles: BTreeMap<String, Palette>,
    loaders: ColorMap,
    sources: ColorMap,
}

impl Config {
    /// Compare effective values without treating a move to custom.toml as an edit.
    pub(crate) fn same_values(&self, other: &Self) -> bool {
        self.settings == other.settings
            && self.styles == other.styles
            && self.loaders == other.loaders
            && self.sources == other.sources
            && self
                .styles
                .keys()
                .all(|id| self.palette_name(id) == other.palette_name(id))
    }

    pub(crate) fn defaults() -> Self {
        Self::parse(
            DEFAULTS
                .into_iter()
                .map(|(name, text)| {
                    (
                        name.to_owned(),
                        toml::from_str(text).expect("valid embedded TOML"),
                    )
                })
                .collect(),
        )
        .expect("embedded theme must be valid")
    }

    pub(crate) fn load(directory: &Path) -> Result<Self> {
        let mut documents = Self::defaults().documents;
        for (name, document) in &mut documents {
            if name.starts_with("styles/") {
                continue;
            }
            let path = directory.join(name);
            match fs::read_to_string(&path) {
                Ok(text) => merge(
                    document,
                    toml::from_str(&text)
                        .with_context(|| format!("invalid TOML in {}", path.display()))?,
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to read {}", path.display()));
                }
            }
        }
        documents.get_mut("settings.toml").unwrap()["settings"]
            .as_table_mut()
            .context("settings must be a table")?
            .remove("appearance");
        loading::styles(&mut documents, directory);
        loading::repair_selections(&mut documents);
        let config = Self::parse(documents)
            .with_context(|| format!("invalid theme configuration in {}", directory.display()))?;
        Ok(config)
    }

    fn parse(documents: BTreeMap<String, Value>) -> Result<Self> {
        #[derive(Deserialize, Debug)]
        struct SettingsFile {
            settings: Settings,
        }
        #[derive(Deserialize, Debug)]
        struct StylesFile {
            style: Vec<Palette>,
        }
        #[derive(Deserialize, Debug)]
        struct LoadersFile {
            loader_colors: ColorMap,
        }
        #[derive(Deserialize, Debug)]
        struct SourcesFile {
            source_colors: ColorMap,
        }
        let settings = documents["settings.toml"]
            .clone()
            .try_into::<SettingsFile>()?
            .settings;
        ensure!(
            settings.version == 1,
            "unsupported settings version {}",
            settings.version
        );
        let mut styles = BTreeMap::new();
        for document in documents
            .values()
            .filter(|document| document.get("style").is_some())
        {
            for palette in document.clone().try_into::<StylesFile>()?.style {
                ensure!(
                    palette.version == 1,
                    "unsupported style version for {}",
                    palette.id
                );
                ensure!(
                    palette
                        .types
                        .iter()
                        .all(|kind| matches!(kind.as_str(), "console" | "tui")),
                    "unknown style type for {}",
                    palette.id
                );
                let id = palette.id.clone();
                ensure!(
                    styles.insert(id.clone(), palette).is_none(),
                    "duplicate style ID {id}"
                );
            }
        }
        for (id, kind) in [
            (&settings.console_style_dark, "console"),
            (&settings.console_style_light, "console"),
            (&settings.tui_style_dark, "tui"),
            (&settings.tui_style_light, "tui"),
        ] {
            let palette = styles
                .get(id)
                .with_context(|| format!("unknown {kind} style {id:?}"))?;
            ensure!(
                palette.types.iter().any(|value| value == kind),
                "style {id:?} does not support {kind}"
            );
        }
        let loaders = documents["loader_colors.toml"]
            .clone()
            .try_into::<LoadersFile>()?
            .loader_colors;
        let sources = documents["source_colors.toml"]
            .clone()
            .try_into::<SourcesFile>()?
            .source_colors;
        Ok(Self {
            documents,
            settings,
            styles,
            loaders,
            sources,
        })
    }

    pub(crate) fn palette(&self, tui: bool) -> &Palette {
        self.palette_for(tui, crate::terminal::light_background().unwrap_or(false))
    }

    pub(crate) fn palette_for(&self, tui: bool, light: bool) -> &Palette {
        let id = match (tui, light) {
            (false, false) => &self.settings.console_style_dark,
            (false, true) => &self.settings.console_style_light,
            (true, false) => &self.settings.tui_style_dark,
            (true, true) => &self.settings.tui_style_light,
        };
        &self.styles[id]
    }

    pub(crate) fn loader_color(&self, tui: bool, id: &str) -> Option<Color> {
        if tui {
            &self.loaders.tui
        } else {
            &self.loaders.console
        }
        .get(id)
        .copied()
    }

    /// Source colors are file-configured identities, outside the palette editor.
    pub(crate) fn source_color(&self, tui: bool, id: &str) -> Option<Color> {
        if tui {
            &self.sources.tui
        } else {
            &self.sources.console
        }
        .get(id)
        .copied()
    }

    /// Stable keys shared by the CLI and interactive editor.
    pub(crate) fn entries(&self) -> Vec<(String, String)> {
        let mut entries = Vec::new();
        for key in [
            "console_style_dark",
            "console_style_light",
            "tui_style_dark",
            "tui_style_light",
        ] {
            let value = &self.documents["settings.toml"]["settings"][key];
            entries.push((
                format!("settings.{key}"),
                value.as_str().unwrap_or("dark").to_owned(),
            ));
        }
        for document in self
            .documents
            .values()
            .filter(|document| document.get("style").is_some())
        {
            for style in document["style"].as_array().expect("validated styles") {
                let id = style["id"].as_str().expect("validated ID");
                for field in ["primary", "secondary"] {
                    entries.push((
                        format!("styles.{id}.{field}"),
                        style[field].as_str().expect("validated color").to_owned(),
                    ));
                }
            }
        }
        for mode in ["console", "tui"] {
            for (id, color) in self.documents["loader_colors.toml"]["loader_colors"][mode]
                .as_table()
                .expect("validated loaders")
            {
                entries.push((
                    format!("loader_colors.{mode}.{id}"),
                    color.as_str().expect("validated color").to_owned(),
                ));
            }
        }
        entries
    }

    pub(crate) fn choices(&self, key: &str) -> Vec<String> {
        if key.starts_with("settings.") {
            let kind = if key.contains("console") {
                "console"
            } else {
                "tui"
            };
            return self
                .styles
                .values()
                .filter(|style| style.types.iter().any(|value| value == kind))
                .map(|style| style.id.clone())
                .collect();
        }
        Color::TUI.into_iter().map(str::to_owned).collect()
    }

    /// Validate a draft before replacing any in-memory or on-disk state.
    pub(crate) fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let mut documents = self.documents.clone();
        if let Some(field) = key.strip_prefix("settings.") {
            ensure!(
                [
                    "console_style_dark",
                    "console_style_light",
                    "tui_style_dark",
                    "tui_style_light"
                ]
                .contains(&field),
                "unknown setting {key:?}"
            );
            documents.get_mut("settings.toml").unwrap()["settings"][field] =
                Value::String(value.into());
        } else if let Some(rest) = key.strip_prefix("styles.") {
            let (id, field) = rest
                .rsplit_once('.')
                .context("expected styles.ID.primary or styles.ID.secondary")?;
            ensure!(
                matches!(field, "primary" | "secondary" | "name" | "types"),
                "unknown style field {field:?}"
            );
            let style = documents
                .values_mut()
                .filter_map(|doc| doc.get_mut("style"))
                .filter_map(Value::as_array_mut)
                .flatten()
                .find(|style| style.get("id").and_then(Value::as_str) == Some(id))
                .with_context(|| format!("unknown style {id:?}"))?;
            if field == "types" {
                let types: Vec<_> = value.split(',').map(str::trim).collect();
                ensure!(
                    !types.is_empty()
                        && types.iter().all(|kind| matches!(*kind, "tui" | "console")),
                    "Types must contain TUI, Console, or both"
                );
                style[field] = Value::Array(
                    types
                        .iter()
                        .map(|kind| Value::String((*kind).into()))
                        .collect(),
                );
                for (_, fallback, kind) in default_selections() {
                    ensure!(
                        id != fallback || types.contains(&kind),
                        "default palette must support {kind}"
                    );
                }
                // Selections must remain compatible when a palette drops a supported type.
                for (key, fallback, kind) in default_selections() {
                    if !types.contains(&kind)
                        && documents["settings.toml"]["settings"][key].as_str() == Some(id)
                    {
                        documents.get_mut("settings.toml").unwrap()["settings"][key] =
                            Value::String(fallback);
                    }
                }
            } else {
                style[field] = Value::String(value.into());
            }
        } else if let Some(rest) = key.strip_prefix("loader_colors.") {
            let (mode, id) = rest
                .split_once('.')
                .context("expected loader_colors.console.ID or loader_colors.tui.ID")?;
            ensure!(
                matches!(mode, "console" | "tui") && !id.is_empty() && !id.contains('.'),
                "invalid loader color key {key:?}"
            );
            documents.get_mut("loader_colors.toml").unwrap()["loader_colors"][mode]
                .as_table_mut()
                .unwrap()
                .insert(id.into(), Value::String(value.into()));
        } else {
            bail!("unknown customization key {key:?}");
        }
        *self = Self::parse(documents)?;
        Ok(())
    }

    /// Only the edited document is replaced, so unrelated user files remain intact.
    pub(crate) fn save_key(&self, directory: &Path, key: &str) -> Result<()> {
        let name = if key.starts_with("settings.") {
            "settings.toml"
        } else if key.starts_with("loader_colors.") {
            "loader_colors.toml"
        } else {
            let id = key
                .strip_prefix("styles.")
                .and_then(|rest| rest.rsplit_once('.'))
                .map(|(id, _)| id)
                .context("invalid style key")?;
            self.documents
                .iter()
                .find(|(_, document)| {
                    document
                        .get("style")
                        .and_then(Value::as_array)
                        .is_some_and(|styles| {
                            styles
                                .iter()
                                .any(|style| style.get("id").and_then(Value::as_str) == Some(id))
                        })
                })
                .map(|(name, _)| name.as_str())
                .context("palette file not found")?
        };
        self.save_document(directory, name)
    }

    pub(super) fn save_document(&self, directory: &Path, name: &str) -> Result<()> {
        let path = directory.join(name);
        let parent = path.parent().context("configuration path has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(toml::to_string_pretty(&self.documents[name])?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(&path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Settings {
    version: u32,
    console_style_dark: String,
    console_style_light: String,
    tui_style_dark: String,
    tui_style_light: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct Palette {
    version: u32,
    id: String,
    types: Vec<String>,
    pub(crate) primary: Color,
    pub(crate) secondary: Color,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct ColorMap {
    console: BTreeMap<String, Color>,
    tui: BTreeMap<String, Color>,
}

/// Bundled selections define fallback IDs; no legacy identifiers are migrated.
fn default_selections() -> [(&'static str, String, &'static str); 4] {
    let document: Value = toml::from_str(DEFAULTS[0].1).expect("valid bundled settings");
    [
        ("console_style_dark", "console"),
        ("console_style_light", "console"),
        ("tui_style_dark", "tui"),
        ("tui_style_light", "tui"),
    ]
    .map(|(key, kind)| {
        (
            key,
            document["settings"][key]
                .as_str()
                .expect("bundled selection")
                .to_owned(),
            kind,
        )
    })
}

pub(crate) fn directory() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".mcpm"))
        .context("cannot locate the user home directory")
}

/// Tables merge recursively; style arrays merge by ID so partial overrides keep defaults.
fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Table(base), Value::Table(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = base.get_mut(&key) {
                    merge(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (Value::Array(base), Value::Array(overlay))
            if overlay
                .iter()
                .all(|item| item.get("id").and_then(Value::as_str).is_some()) =>
        {
            for value in overlay {
                let id = value.get("id").and_then(Value::as_str);
                if let Some(existing) = base
                    .iter_mut()
                    .find(|item| id.is_some() && item.get("id").and_then(Value::as_str) == id)
                {
                    merge(existing, value);
                } else {
                    base.push(value);
                }
            }
        }
        (base, value) => *base = value,
    }
}
