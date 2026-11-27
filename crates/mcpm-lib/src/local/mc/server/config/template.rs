//! `toml_edit` AST-backed renderer for `conf.template.toml`.
//!
//! `conf.template.toml` is maintained as a complete, valid, uncommented example TOML document
//! (self-validated by tests against [`super::decode_conf`]). Rendering walks its AST in exact
//! source order, keeping required/selected settings active with their comments preserved and
//! commenting out every unselected table, array-of-tables, and key in place.

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Key, Table, Value};

use super::{ConfigurationOverrides, ServerConfig, decode_conf_with_overrides};

/// Valid uncommented example template; every schema field and comment lives here once.
pub(super) const CONF_TEMPLATE: &str = include_str!("conf.template.toml");

/// Default `[[restart_on]]` rule enabled in newly generated `conf.toml` files.
const DEFAULT_ENABLED_RESTART_RULE: &str = "watchdog";

/// Renders the default configuration with the required `[java]` table and the default
/// `watchdog` `[[restart_on]]` rule active, while keeping all other optional settings and
/// restart examples commented out in place.
pub(super) fn default_template() -> String {
    render_template::<&toml::Value>(BTreeMap::new())
        .expect("embedded conf.template.toml must render cleanly")
}

/// Renders explicit launch/init overrides into the default template while preserving all comments.
pub(super) fn manual_template(
    overrides: &ConfigurationOverrides,
) -> Result<(String, ServerConfig)> {
    let conf = decode_conf_with_overrides(&default_template(), overrides)?;
    let schema = toml::Value::try_from(&conf)?;
    // A list may be modified more than once; only its final value is emitted.
    let mut settings = BTreeMap::new();
    for key in overrides.keys() {
        if let std::collections::btree_map::Entry::Vacant(entry) = settings.entry(key) {
            let value = key
                .split('.')
                .try_fold(&schema, |table, part| table.get(part))
                .with_context(|| format!("cannot encode configured field {key}"))?;
            entry.insert(value);
        }
    }
    Ok((render_template(settings)?, conf))
}

/// Renders imported settings (or generated defaults such as `server.nogui = true`) into the
/// template, failing if any setting is duplicated or has no matching field in `conf.template.toml`.
pub(super) fn render_suggestions<'a, T: Serialize>(
    suggestions: impl IntoIterator<Item = (&'a str, T)>,
) -> Result<String> {
    let mut settings = BTreeMap::new();
    for (key, value) in suggestions {
        ensure!(
            settings.insert(key, value).is_none(),
            "repeated imported setting {key} cannot be represented as one setting"
        );
    }
    render_template(settings)
}

/// Encodes one TOML value via the canonical `toml` serializer.
pub(super) fn encode_setting(value: &impl Serialize) -> Result<String> {
    #[derive(Serialize)]
    struct Wrapper<'a, T> {
        value: &'a T,
    }
    let text = toml::to_string(&Wrapper { value })?;
    Ok(text
        .strip_prefix("value = ")
        .context("unexpected TOML value encoding")?
        .trim_end()
        .to_owned())
}

fn render_template<T: Serialize>(mut remaining: BTreeMap<&str, T>) -> Result<String> {
    // Normalize CRLF on Windows checkouts so rendered conf.toml never mixes \r\n and \n.
    let normalized;
    let source = if CONF_TEMPLATE.contains('\r') {
        normalized = CONF_TEMPLATE.replace("\r\n", "\n");
        normalized.as_str()
    } else {
        CONF_TEMPLATE
    };
    let doc: DocumentMut = source
        .parse()
        .context("embedded conf.template.toml is not valid TOML")?;

    let mut sections = Vec::new();
    let mut path = Vec::new();
    let mut last_position = 0isize;
    collect_sections(doc.as_table(), &mut path, false, &mut last_position, &mut sections);
    sections.sort_by_key(|section| (!section.path.is_empty(), section.position));

    let mut output = String::with_capacity(CONF_TEMPLATE.len() + 256);
    if let Some(prefix) = doc.decor().prefix().and_then(|raw| raw.as_str()) {
        output.push_str(prefix);
    }

    for section in sections {
        render_section(&section, &mut remaining, &mut output)?;
    }

    if let Some(suffix) = doc.decor().suffix().and_then(|raw| raw.as_str()) {
        output.push_str(suffix);
    }
    if let Some(trailing) = doc.trailing().as_str() {
        output.push_str(trailing);
    }

    ensure!(
        remaining.is_empty(),
        "unsupported template fields: {:?}",
        remaining.keys()
    );
    Ok(output)
}

struct Section<'a> {
    position: isize,
    table: &'a Table,
    path: Vec<&'a Key>,
    is_array_of_tables: bool,
}

fn collect_sections<'a>(
    table: &'a Table,
    path: &mut Vec<&'a Key>,
    is_array_of_tables: bool,
    last_position: &mut isize,
    sections: &mut Vec<Section<'a>>,
) {
    if !table.is_dotted() {
        if let Some(pos) = table.position() {
            *last_position = pos;
        }
        sections.push(Section {
            position: *last_position,
            table,
            path: path.clone(),
            is_array_of_tables,
        });
    }

    for (key, item) in table.iter() {
        let Some((key_ref, _)) = table.get_key_value(key) else {
            continue;
        };
        match item {
            Item::Table(sub) => {
                path.push(key_ref);
                collect_sections(sub, path, false, last_position, sections);
                path.pop();
            }
            Item::ArrayOfTables(array) => {
                for element in array.iter() {
                    path.push(key_ref);
                    collect_sections(element, path, true, last_position, sections);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

fn render_section<T: Serialize>(
    section: &Section<'_>,
    remaining: &mut BTreeMap<&str, T>,
    output: &mut String,
) -> Result<()> {
    let entries = section.table.get_values();
    let visible_table = !(section.table.is_implicit() && entries.is_empty());

    let dotted_path = join_key_names(&section.path);
    let header_display = join_key_reprs(&section.path);
    let active_array_table = section.is_array_of_tables
        && dotted_path == "restart_on"
        && section
            .table
            .get("name")
            .and_then(Item::as_str)
            == Some(DEFAULT_ENABLED_RESTART_RULE);

    if !section.path.is_empty() && (section.is_array_of_tables || visible_table) {
        if let Some(prefix) = section.table.decor().prefix().and_then(|raw| raw.as_str()) {
            output.push_str(prefix);
        }
        let suffix = section
            .table
            .decor()
            .suffix()
            .and_then(|raw| raw.as_str())
            .unwrap_or("");
        let table_active = active_array_table
            || (!section.is_array_of_tables
                && (dotted_path == "java"
                    || entries.iter().any(|(key_path, _)| {
                        let full = entry_path(&dotted_path, key_path);
                        remaining.contains_key(full.as_str())
                    })));
        let header = if section.is_array_of_tables {
            format!("[[{header_display}]]{suffix}")
        } else {
            format!("[{header_display}]{suffix}")
        };
        if !table_active {
            output.push_str("# ");
        }
        output.push_str(&header);
        output.push('\n');
    }

    for (key_path, value) in entries {
        let leaf = key_path.last().expect("nonempty key path");
        if let Some(prefix) = leaf.leaf_decor().prefix().and_then(|raw| raw.as_str()) {
            output.push_str(prefix);
        }

        let key_repr = join_key_reprs(&key_path);
        let full_path = entry_path(&dotted_path, &key_path);

        if active_array_table {
            let key_suffix = leaf
                .leaf_decor()
                .suffix()
                .and_then(|raw| raw.as_str())
                .unwrap_or(" ");
            output.push_str(&format!("{key_repr}{key_suffix}={value}\n"));
        } else if !section.is_array_of_tables
            && let Some(configured) = remaining.remove(full_path.as_str())
        {
            let encoded = encode_setting(&configured)?;
            let descriptions = extract_descriptions(value);
            if !encoded.contains('\n') && descriptions.len() <= 1 {
                output.push_str(&key_repr);
                output.push_str(" = ");
                output.push_str(&encoded);
                if let Some(desc) = descriptions.first() {
                    output.push_str("  ");
                    output.push_str(desc);
                }
                output.push('\n');
            } else {
                output.push_str(&key_repr);
                output.push_str(" = ");
                output.push_str(&encoded);
                output.push('\n');
                for desc in descriptions {
                    output.push_str(desc);
                    output.push('\n');
                }
            }
        } else {
            let key_suffix = leaf
                .leaf_decor()
                .suffix()
                .and_then(|raw| raw.as_str())
                .unwrap_or(" ");
            let raw = format!("{key_repr}{key_suffix}={value}");
            for line in raw.lines() {
                output.push_str("# ");
                output.push_str(line);
                output.push('\n');
            }
        }
    }

    Ok(())
}

fn join_key_names(path: &[&Key]) -> String {
    let mut out = String::new();
    for (index, key) in path.iter().enumerate() {
        if index > 0 {
            out.push('.');
        }
        out.push_str(key.get());
    }
    out
}

fn join_key_reprs(path: &[&Key]) -> String {
    let mut out = String::new();
    for (index, key) in path.iter().enumerate() {
        if index > 0 {
            out.push('.');
        }
        out.push_str(&key.display_repr());
    }
    out
}

fn entry_path(table_path: &str, key_path: &[&Key]) -> String {
    let key_name = join_key_names(key_path);
    if table_path.is_empty() {
        key_name
    } else {
        format!("{table_path}.{key_name}")
    }
}

fn extract_descriptions(value: &Value) -> Vec<&str> {
    let mut descriptions = Vec::new();
    if let Value::Array(array) = value
        && let Some(first) = array.iter().next()
        && let Some(prefix) = first.decor().prefix().and_then(|raw| raw.as_str())
        && let Some(first_line) = prefix.lines().next()
    {
        let trimmed = first_line.trim();
        if trimmed.starts_with('#') {
            descriptions.push(trimmed);
        }
    }
    if let Some(suffix) = value.decor().suffix().and_then(|raw| raw.as_str()) {
        let trimmed = suffix.trim();
        if trimmed.starts_with('#') {
            descriptions.push(trimmed);
        }
    }
    descriptions
}
