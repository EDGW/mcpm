use std::{path::Path, process::Command};

#[test]
fn client_list_reports_loaders_and_broken_profiles_in_raw_and_redirected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clients");
    let mut outputs = Vec::new();
    for raw in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm"));
        command.current_dir(&root);
        if raw {
            command.arg("--raw");
        }
        let output = command.args(["client", "list"]).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<Vec<_>> = stdout
            .lines()
            .map(|line| line.split('\t').collect())
            .collect();
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|row| row.len() == 5));
        assert_eq!(&rows[0][..4], ["1", "broken", "broken", ""]);
        assert!(rows[0][4].contains("failed to parse Minecraft version manifest"));
        assert_eq!(rows[1], ["2", "fabric", "normal", "Fabric 0.19.3", ""]);
        assert_eq!(rows[2], ["3", "forge", "normal", "Forge 47.4.16", ""]);
        assert_eq!(&rows[3][..4], ["4", "missing", "broken", ""]);
        assert!(rows[3][4].contains("failed to read Minecraft version manifest"));
        assert_eq!(
            rows[4],
            ["5", "neoforge", "normal", "NeoForge 21.1.227", ""]
        );
        assert_eq!(rows[5], ["6", "vanilla", "normal", "", "vanilla"]);
        outputs.push(stdout);
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn client_list_handles_explicit_root_empty_results_and_input_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clients");
    let run = |path: &Path| {
        Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args(["client", "list"])
            .arg(path)
            .output()
            .unwrap()
    };
    let empty = run(&root.join("versions/vanilla"));
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty());
    let missing = run(&root.join("does-not-exist"));
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("error:"));
    assert!(run(&root).status.success());
}

#[test]
fn client_list_infers_last_minecraft_component_only_for_omitted_paths() {
    let outer = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/client-path/.minecraft");
    let inner = outer.join("versions/outer/.minecraft");
    let current = inner.join("versions/inner/.minecraft-backup");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .current_dir(&current)
            .args(["client", "list"])
            .args(args)
            .output()
            .unwrap()
    };

    let inferred = run(&[]);
    assert!(inferred.status.success());
    assert_eq!(
        String::from_utf8(inferred.stdout).unwrap(),
        "1\tinner\tnormal\t\tvanilla\n"
    );
    assert!(inferred.stderr.is_empty());

    let explicit_current = run(&["."]);
    assert!(explicit_current.status.success());
    assert!(explicit_current.stdout.is_empty());
    assert!(explicit_current.stderr.is_empty());

    let explicit_outer = run(&[outer.to_str().unwrap()]);
    assert!(explicit_outer.status.success());
    assert_eq!(
        String::from_utf8(explicit_outer.stdout).unwrap(),
        "1\touter\tnormal\t\tvanilla\n"
    );
    assert!(explicit_outer.stderr.is_empty());

    let raw = run(&["--raw"]);
    assert!(raw.status.success());
    assert_eq!(
        String::from_utf8(raw.stdout).unwrap(),
        "1\tinner\tnormal\t\tvanilla\n"
    );
    assert!(raw.stderr.is_empty());
}
