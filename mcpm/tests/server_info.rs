use std::{path::Path, process::Command};

#[test]
fn server_info_reports_metadata_with_explicit_and_default_paths() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/servers");
    for (name, version, loader, loader_version) in [
        ("forge", "1.20.1", "Forge", "47.3.12"),
        ("vanilla", "1.21.1", "vanilla or unrecognized", ""),
        ("unknown", "1.21.1", "vanilla or unrecognized", ""),
    ] {
        let root = fixtures.join(name);
        for raw in [false, true] {
            for explicit in [false, true] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm"));
                command.current_dir(&root);
                if raw {
                    command.arg("--raw");
                }
                command.args(["server", "info"]);
                if explicit {
                    command.arg(&root);
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(output.stderr.is_empty());
                let path = if explicit {
                    root.display().to_string()
                } else {
                    ".".into()
                };
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap(),
                    format!("{name}\t{path}\t{version}\t{loader}\t{loader_version}\tunavailable\n")
                );
            }
        }
    }
}

#[test]
fn server_info_rejects_missing_metadata() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/servers");
    let output = Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args(["server", "info"])
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("failed to read Minecraft server metadata")
    );
    assert!(!root.join(".mcpm").exists());
}
