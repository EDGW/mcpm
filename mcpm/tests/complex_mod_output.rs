//! Real CLI and parser coverage for composite archives and their list output.

use std::path::Path;
use std::process::Command;

use mcpm_backend::local::{
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
        &mcpm_backend::local::runtime::RuntimeEnvironment::new().with_mods_cache(None),
        |_, _, _, _| IdentificationErrorAction::Abort,
    )
    .expect("failed to identify composite archive")
    .file;
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
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
        process.current_dir(&fixtures);
        process.arg("--raw");
        let output = process
            .args(["mod", command, "complex-mod.jar", "--no-cache"])
            .args(options)
            .output()
            .expect("failed to run mcpm");
        let actual = String::from_utf8(output.stdout).expect("output must be UTF-8");
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(actual, expected, "{command}, options={options:?}");
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
