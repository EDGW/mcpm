//! Human-readable customization fields, separate from persistence keys.

use crate::theme::{Color, config::Config};

#[derive(Debug)]
pub(super) struct Field {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) value: String,
}

pub(super) fn fields(config: &Config, editing: bool) -> Vec<Field> {
    let mut fields: Vec<Field> = CATEGORY_KEYS
        .iter()
        .map(|key| (key, selected_id(config, key)))
        .enumerate()
        .map(|(index, (key, id))| Field {
            key: (*key).into(),
            label: CATEGORIES[index].into(),
            value: config.palette_name(&id),
        })
        .collect();
    if editing {
        let selected = config.selected_palettes();
        for (key, value) in config.entries() {
            if let Some(rest) = key.strip_prefix("styles.") {
                let Some((id, field)) = rest.rsplit_once('.') else {
                    continue;
                };
                if let Some(index) = selected.iter().position(|selected| selected == id) {
                    let section = ["Console Dark", "Console Light", "TUI Dark", "TUI Light"][index];
                    fields.push(Field {
                        key: key.clone(),
                        label: format!("{section} {} Color", title(field)),
                        value: Color::parse(&value)
                            .map(|color| color.tui_name().to_owned())
                            .unwrap_or(value),
                    });
                }
            } else if let Some(rest) = key.strip_prefix("loader_colors.") {
                let (mode, loader) = rest.split_once('.').unwrap();
                let mode = if mode == "tui" { "TUI" } else { "Console" };
                fields.push(Field {
                    key: key.clone(),
                    label: format!("{mode} {} Color", title(loader)),
                    value: Color::parse(&value)
                        .map(|color| color.tui_name().to_owned())
                        .unwrap_or(value),
                });
            }
        }
    }
    fields
}

pub(super) fn title(value: &str) -> String {
    match value {
        "fabric" => "Fabric".into(),
        "forge" => "Forge".into(),
        "neoforge" => "NeoForge".into(),
        "default_color" | "Reset" => "Default Foreground".into(),
        _ => value
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub(super) const CATEGORY_KEYS: [&str; 4] = [
    "settings.tui_style_dark",
    "settings.tui_style_light",
    "settings.console_style_dark",
    "settings.console_style_light",
];
pub(super) const CATEGORIES: [&str; 4] = ["TUI Dark", "TUI Light", "Console Dark", "Console Light"];

pub(super) fn edit_fields(config: &Config, id: &str, type_cursor: usize) -> Vec<Field> {
    let types = config.palette_types(id);
    let types = ["tui", "console"]
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            format!(
                "{}[{}] {}",
                if index == type_cursor { ">" } else { " " },
                if types.iter().any(|value| value == kind) {
                    "x"
                } else {
                    " "
                },
                if *kind == "tui" { "TUI" } else { "Console" }
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    vec![
        Field {
            key: format!("styles.{id}.name"),
            label: "Name".into(),
            value: config.palette_name(id),
        },
        Field {
            key: format!("styles.{id}.types"),
            label: "Types".into(),
            value: types,
        },
        Field {
            key: format!("styles.{id}.primary"),
            label: "Primary".into(),
            value: config.palette_primary(id).tui_name().into(),
        },
        Field {
            key: format!("styles.{id}.secondary"),
            label: "Secondary".into(),
            value: config.palette_secondary(id).tui_name().into(),
        },
    ]
}

pub(super) fn selected_id(config: &Config, key: &str) -> String {
    config
        .entries()
        .into_iter()
        .find(|(entry, _)| entry == key)
        .expect("known palette selection")
        .1
}
