use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// Exercise the real clap dispatch, all four parsers, nested archives, and output.
#[test]
fn complex_mod_info_and_deps_match_reviewed_output() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (command, expected) in [
        ("info", include_str!("fixtures/complex-mod.info.txt")),
        ("deps", include_str!("fixtures/complex-mod.deps.txt")),
    ] {
        for raw in [false, true] {
            let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"));
            process.current_dir(&fixtures);
            if raw {
                process.arg("--raw");
            }
            let output = process
                .args(["mod", command, "complex-mod.jar"])
                .output()
                .expect("failed to run mcpm");
            let actual = String::from_utf8(output.stdout).expect("output must be UTF-8");
            assert!(
                output.status.success(),
                "{command}, raw={raw}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "unexpected diagnostics for {command}"
            );
            assert_eq!(actual, expected, "{command}, raw={raw}");
        }
    }

    // Preview only after both commands have passed every output comparison.
    for command in ["info", "deps"] {
        println!("\n--- verified mcpm mod {command}: console preview ---");
        io::stdout()
            .flush()
            .expect("failed to flush preview heading");
        let status = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .current_dir(&fixtures)
            .args(["mod", command, "complex-mod.jar"])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .expect("failed to run console preview");
        assert!(status.success(), "console preview failed for {command}");
    }
}
