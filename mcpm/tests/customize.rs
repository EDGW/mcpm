//! Customization CLI contract: redirected/raw output, persistence and rejected edits.

use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn commands_persist_values_and_redirected_output_matches_raw() {
    let home = tempfile::tempdir().unwrap();
    let list = run(home.path(), &["customize", "--no-ui", "list"]);
    let raw = run(home.path(), &["--raw", "customize", "list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert_eq!(list.stdout, raw.stdout);
    assert!(list.stderr.is_empty());
    assert!(String::from_utf8_lossy(&list.stdout).contains("Console Dark Primary Color\tCyan\n"));
    assert!(home.path().join(".mcpm/logs").is_dir());
    assert!(!home.path().join(".mcpm/settings.toml").exists());
    assert!(!home.path().join(".mcpm/styles").exists());
    let set = run(
        home.path(),
        &[
            "customize",
            "set",
            "styles.default_dark.primary",
            "bright_magenta",
        ],
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert_eq!(set.stdout, b"Console Dark Primary Color\tMagenta\n");
    let get = run(
        home.path(),
        &["customize", "get", "styles.default_dark.primary"],
    );
    assert!(get.status.success());
    assert_eq!(get.stdout, set.stdout);
    let path = home.path().join(".mcpm/styles/tui.toml");
    let before = std::fs::read(&path).unwrap();
    let invalid = run(
        home.path(),
        &["customize", "set", "styles.default_dark.primary", "invalid"],
    );
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unknown terminal color"));
    assert_eq!(std::fs::read(path).unwrap(), before);
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
fn schemes_copy_selected_palette_and_survive_rename_select_and_delete() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["customize", "scheme", "create", "ocean", "--name", "Ocean"],
        vec!["customize", "scheme", "rename", "ocean", "Ocean Blue"],
        vec!["customize", "scheme", "use", "default_dark"],
        vec!["customize", "scheme", "use", "ocean"],
    ] {
        let output = run(home.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run(home.path(), &["customize", "list"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Console Dark\tOcean Blue"));
    let deleted = run(home.path(), &["customize", "scheme", "delete", "ocean"]);
    assert!(
        deleted.status.success(),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    let output = run(home.path(), &["customize", "list"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Console Dark\tDefault Dark"));
    assert!(
        !run(
            home.path(),
            &["customize", "scheme", "delete", "default_dark"]
        )
        .status
        .success()
    );
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
    assert!(String::from_utf8_lossy(&list.stderr).contains("WARN"));
    let edit = run(
        home.path(),
        &[
            "--log-level",
            "off",
            "customize",
            "set",
            "styles.ocean.primary",
            "bright_magenta",
        ],
    );
    assert!(
        edit.status.success(),
        "{}",
        String::from_utf8_lossy(&edit.stderr)
    );
    assert!(edit.stderr.is_empty());
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

#[test]
fn ids_are_immutable_and_independent_of_display_names() {
    let home = tempfile::tempdir().unwrap();
    let create = run(home.path(), &["customize", "scheme", "create", "my-theme"]);
    assert!(create.status.success());
    assert!(String::from_utf8_lossy(&create.stdout).contains("my-theme\tmy-theme"));
    for args in [
        vec!["customize", "scheme", "create", "my-theme"],
        vec!["customize", "scheme", "create", "invalid.id"],
        vec!["customize", "set", "styles.my-theme.id", "another-id"],
    ] {
        assert!(!run(home.path(), &args).status.success());
    }
    let rename = run(
        home.path(),
        &["customize", "scheme", "rename", "my-theme", "My Theme"],
    );
    assert!(rename.status.success());
    let list = run(home.path(), &["customize", "scheme", "list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("my-theme\tMy Theme"));
    let data = std::fs::read_to_string(home.path().join(".mcpm/styles/custom.toml")).unwrap();
    assert!(data.contains("id = \"my-theme\""));
    assert!(data.contains("name = \"My Theme\""));
}
