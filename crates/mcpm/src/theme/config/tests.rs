//! Regression coverage for consistent palette decoding across file loading and edits.

use super::*;

#[test]
fn invalid_palette_entries_follow_the_same_rules_in_loading_and_configuration_building() {
    let defaults = Config::defaults();
    let original = defaults.documents["styles/tui.toml"]["style"][0].clone();
    for (field, value) in [
        ("id", Value::String("invalid.id".into())),
        ("name", Value::String(" ".into())),
        ("name", Value::String("line\nbreak".into())),
        ("name", Value::Integer(1)),
        ("types", Value::Array(Vec::new())),
        ("types", Value::Array(vec![Value::String("unknown".into())])),
        ("version", Value::Integer(2)),
        ("primary", Value::String("unknown".into())),
    ] {
        let mut entry = original.clone();
        entry["id"] = Value::String("imported".into());
        entry[field] = value;
        let mut document = Value::Table(Default::default());
        document
            .as_table_mut()
            .unwrap()
            .insert("style".into(), Value::Array(vec![entry]));
        let mut documents = defaults.documents.clone();
        documents.insert("styles/imported.toml".into(), document.clone());
        assert!(Config::parse(documents).is_err(), "{field}: {document}");

        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("styles")).unwrap();
        fs::write(
            root.path().join("styles/imported.toml"),
            toml::to_string(&document).unwrap(),
        )
        .unwrap();
        let loaded = Config::load(root.path()).unwrap();
        assert!(loaded.same_values(&defaults), "{field}: {document}");
    }
}

#[test]
fn invalid_name_edits_are_atomic_and_partial_file_overrides_use_the_effective_palette() {
    let mut config = Config::defaults();
    config
        .create_palette("settings.tui_style_dark", "ocean")
        .unwrap();
    let original = config.clone();
    for name in ["", " ", "line\nbreak"] {
        assert!(config.set("styles.ocean.name", name).is_err());
        assert_eq!(config.documents, original.documents);
        assert!(config.rename_palette("ocean", name).is_err());
        assert_eq!(config.documents, original.documents);
    }
    for types in ["", "unknown"] {
        assert!(config.set("styles.ocean.types", types).is_err());
        assert_eq!(config.documents, original.documents);
    }
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("styles")).unwrap();
    fs::write(
        root.path().join("styles/override.toml"),
        "[[style]]\nid='default_dark'\nname='Ocean'\nprimary='Blue'\n",
    )
    .unwrap();
    let loaded = Config::load(root.path()).unwrap();
    assert_eq!(loaded.palette_name("default_dark"), "Ocean");
    assert_eq!(loaded.palette(false).primary.tui_name(), "Blue");
    assert_eq!(
        loaded.palette(false).secondary,
        Config::defaults().palette(false).secondary
    );
    config.set("styles.ocean.name", "Ocean").unwrap();
    assert_eq!(config.palette_name("ocean"), "Ocean");

    let mut documents = config.documents.clone();
    for document in documents.values_mut() {
        if let Some(styles) = document.get_mut("style").and_then(Value::as_array_mut) {
            for style in styles {
                if style["id"].as_str() == Some("ocean") {
                    style.as_table_mut().unwrap().remove("name");
                }
            }
        }
    }
    let mut unnamed = Config::parse(documents).unwrap();
    assert_eq!(unnamed.palette_name("ocean"), "Unnamed Color Scheme");
    unnamed.rename_palette("ocean", "Named").unwrap();
    assert_eq!(unnamed.palette_name("ocean"), "Named");
}
