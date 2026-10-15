use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry, ModMeta},
    modloader::BUILTIN_LOADERS,
};

/// Exercise the real clap dispatch, all four parsers, nested archives, and output.
#[test]
fn complex_mod_info_and_deps_match_reviewed_output() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let file = mcmod::open(
        ModEntry::new(fixtures.join("complex-mod.jar")).unwrap(),
        BUILTIN_LOADERS.clone(),
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .expect("failed to identify composite archive");
    assert_eq!(file.metadata.len(), 4);
    for metadata in &file.metadata {
        let fml = matches!(metadata.loader.id.as_str(), "forge" | "neoforge");
        assert_eq!(metadata.mods.len(), if fml { 2 } else { 1 });
        assert_eq!(metadata.nested.len(), 3);
        assert_eq!(metadata.nested[0].path, "META-INF/jars/child.jar");
        assert_eq!(
            metadata.nested[0].meta.nested[0].path,
            "META-INF/jars/grandchild.jar"
        );
        assert_eq!(metadata.nested[1].meta.mods.len(), if fml { 3 } else { 1 });
        assert!(
            !metadata.nested[2].meta.has_mods(),
            "plain library must be retained without inventing mods"
        );
        assert_loader_tree(metadata, &metadata.loader.id);
    }

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

fn assert_loader_tree(metadata: &ModMeta, loader: &str) {
    assert_eq!(metadata.loader.id, loader);
    for child in &metadata.nested {
        assert!(!child.path.starts_with('/'));
        assert!(!child.path.contains('!'));
        assert_loader_tree(&child.meta, loader);
    }
}
