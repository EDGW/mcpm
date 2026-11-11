//! Legacy registrations cannot drive server commands or leak partial Raw result rows.

use std::{fs, process::Command};

use serde_json::json;

#[test]
fn raw_server_commands_reject_legacy_registration_before_other_preparation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    let path = root.path().join(".mcpm/server.json");
    let bytes = serde_json::to_vec(&json!({
        "version":3,"minecraft_version":"1.12.2",
        "loader":{"type":"fabric","version":"0.16.10"}
    }))
    .unwrap();
    fs::write(&path, &bytes).unwrap();
    fs::write(root.path().join("launch.toml"), "invalid unrelated TOML [").unwrap();
    fs::write(
        root.path().join("conf.toml"),
        "[java]\nexecutable='not-installed-java'\n",
    )
    .unwrap();
    for args in [
        vec!["info"],
        vec!["info", "--full-command"],
        vec!["info", "--upgrade"],
        vec!["launch", "--detach"],
        vec!["attach"],
        vec!["stash", "status"],
        vec!["mods", "list"],
        vec![
            "init",
            "--force",
            "--minecraft-version",
            "1.12.2",
            "--loader",
            "vanilla",
            "--no-git",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .arg("--raw")
            .args(&args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("info --upgrade"),
            "{args:?}: {output:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes, "{args:?}");
        assert!(!root.path().join("mods").exists());
        assert!(!root.path().join(".mcpm/run").exists());
        assert!(!root.path().join(".git").exists());
    }
}
