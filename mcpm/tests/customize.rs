//! Custom style discovery, persistence and isolation between collections.

use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn deleting_imported_schemes_survives_restart_without_rewriting_collections() {
    let home = tempfile::tempdir().unwrap();
    let styles = home.path().join(".mcpm/styles");
    std::fs::create_dir_all(&styles).unwrap();
    let imported = "[[style]]\nversion = 1\nid = 'imported'\nname = 'Imported'\ntypes = ['tui', 'console']\nprimary = 'Cyan'\nsecondary = 'Blue'\n";
    std::fs::write(styles.join("imported.toml"), imported).unwrap();
    let id = "imported";
    let selected = run(home.path(), &["customize", "scheme", "use", id]);
    assert!(selected.status.success());
    let deleted = run(home.path(), &["customize", "scheme", "delete", id]);
    assert!(
        deleted.status.success(),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    let reloaded = run(home.path(), &["customize", "scheme", "list"]);
    assert!(reloaded.status.success());
    assert!(
        !String::from_utf8_lossy(&reloaded.stdout)
            .lines()
            .any(|line| line.starts_with(&format!("{id}\t")))
    );
    assert_eq!(
        std::fs::read_to_string(styles.join("imported.toml")).unwrap(),
        imported
    );
    let builtin = run(
        home.path(),
        &["customize", "scheme", "delete", "default_red"],
    );
    assert!(!builtin.status.success());
    let recreated = run(home.path(), &["customize", "scheme", "create", "imported"]);
    assert!(recreated.status.success());
    let reloaded = run(home.path(), &["customize", "scheme", "list"]);
    assert!(String::from_utf8_lossy(&reloaded.stdout).contains("imported\t"));
}

#[test]
fn style_discovery_skips_bad_files_and_entries_and_saves_to_original_collection() {
    let home = tempfile::tempdir().unwrap();
    let styles = home.path().join(".mcpm/styles");
    std::fs::create_dir_all(&styles).unwrap();
    std::fs::write(styles.join("broken.toml"), "bad = [").unwrap();
    let path = styles.join("ocean.toml");
    std::fs::write(&path, "[[style]]\nversion = 1\nid = 'ocean'\nname = 'Ocean'\ntypes = ['console']\nprimary = 'bright_blue'\nsecondary = 'bright_cyan'\n\n[[style]]\nid = 'broken'\nprimary = 'invalid'\n").unwrap();
    let list = run(home.path(), &["customize", "scheme", "list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(String::from_utf8_lossy(&list.stdout).contains("ocean\tOcean"));
    let edit = run(
        home.path(),
        &["customize", "set", "styles.ocean.primary", "bright_magenta"],
    );
    assert!(
        edit.status.success(),
        "{}",
        String::from_utf8_lossy(&edit.stderr)
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("bright_magenta")
    );
    assert_eq!(
        std::fs::read_to_string(styles.join("broken.toml")).unwrap(),
        "bad = ["
    );
}

#[test]
fn panel_edits_use_custom_collection_without_rewriting_source_styles() {
    let home = tempfile::tempdir().unwrap();
    let styles = home.path().join(".mcpm/styles");
    std::fs::create_dir_all(&styles).unwrap();
    let source = "# Keep this collection untouched\n[[style]]\nversion = 1\nid = 'ocean'\nname = 'Ocean'\ntypes = ['console']\nprimary = 'blue'\nsecondary = 'cyan'\n";
    let path = styles.join("z-ocean.toml");
    std::fs::write(&path, source).unwrap();
    let rename = run(
        home.path(),
        &["customize", "scheme", "rename", "ocean", "Ocean Edited"],
    );
    assert!(
        rename.status.success(),
        "{}",
        String::from_utf8_lossy(&rename.stderr)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    assert!(!styles.join("console.toml").exists());
    assert!(!styles.join("tui.toml").exists());
    assert!(
        std::fs::read_to_string(styles.join("custom.toml"))
            .unwrap()
            .contains("Ocean Edited")
    );
    let list = run(home.path(), &["customize", "scheme", "list"]);
    assert!(list.status.success());
    assert!(
        list.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(String::from_utf8_lossy(&list.stdout).contains("ocean\tOcean Edited"));
}
