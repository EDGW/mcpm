//! Real CLI and parser coverage for composite archives and their list output.

use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use kako_mods_lib::local::{
    mcmod::{self, IdentificationErrorAction, ModEntry, ModMeta},
    modloader::BUILTIN_LOADERS,
};

/// Exercise the real clap dispatch, all three parsers, nested archives, and output.
#[test]
fn complex_mod_tree_and_output_match_reviewed_expectations() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let file = mcmod::open(
        ModEntry::new(fixtures.join("complex-mod.jar")).unwrap(),
        BUILTIN_LOADERS.clone(),
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .expect("failed to identify composite archive");
    assert_eq!(file.metadata.len(), 3);
    for metadata in &file.metadata {
        let fml = matches!(metadata.loader.id().as_str(), "forge" | "neoforge");
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
        assert_loader_tree(metadata, metadata.loader.id().as_str());
    }

    for (command, options, expected) in [
        (
            "info",
            &[][..],
            include_str!("fixtures/complex-mod.info.txt"),
        ),
        (
            "deps",
            &[][..],
            include_str!("fixtures/complex-mod.deps.txt"),
        ),
        (
            "list",
            &["--expanded", "-C", "IMFL"][..],
            include_str!("fixtures/complex-mod.list.txt"),
        ),
        (
            "list",
            &["--expanded", "--loader", "fabric", "-C", "IMFL"][..],
            include_str!("fixtures/complex-mod.list.fabric.txt"),
        ),
        (
            "list",
            &["--expanded", "--index-type", "tree", "-C", "INMFL"][..],
            include_str!("fixtures/complex-mod.list.tree.txt"),
        ),
        (
            "list",
            &[
                "--expanded",
                "--index-type",
                "tree",
                "--loader",
                "fabric",
                "-C",
                "INMFL",
            ][..],
            include_str!("fixtures/complex-mod.list.tree.fabric.txt"),
        ),
        (
            "list",
            &[
                "--expanded",
                "--index-type",
                "tree",
                "--loader",
                "neoforge",
                "-C",
                "INMFL",
            ][..],
            include_str!("fixtures/complex-mod.list.tree.neoforge.txt"),
        ),
    ] {
        for raw in [false, true] {
            let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"));
            process.current_dir(&fixtures);
            if raw {
                process.arg("--raw");
            }
            let output = process
                .args(["mod", command, "complex-mod.jar"])
                .args(options)
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
            assert_eq!(
                actual, expected,
                "{command}, options={options:?}, raw={raw}"
            );
        }
    }

    // Preview only after every command has passed every output comparison.
    for (command, index_type) in [
        ("info", None),
        ("deps", None),
        ("list", Some("flat")),
        ("list", Some("tree")),
    ] {
        println!(
            "\n--- verified mcpm mod {command}, index_type={index_type:?}: console preview ---"
        );
        io::stdout()
            .flush()
            .expect("failed to flush preview heading");
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"));
        process
            .current_dir(&fixtures)
            .args(["mod", command, "complex-mod.jar"]);
        if let Some(index_type) = index_type {
            process.args(["--no-ui", "--expanded", "--index-type", index_type]);
        }
        let status = process
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .expect("failed to run console preview");
        assert!(status.success(), "console preview failed for {command}");
    }
}

fn assert_loader_tree(metadata: &ModMeta, loader: &str) {
    assert_eq!(metadata.loader.id().as_str(), loader);
    for child in &metadata.nested {
        assert!(!child.path.starts_with('/'));
        assert!(!child.path.contains('!'));
        assert_loader_tree(&child.meta, loader);
    }
}
