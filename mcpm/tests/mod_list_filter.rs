//! Real CLI coverage for filter validation, hidden declarations and independent server inputs.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn filters_find_hidden_declarations_before_compacting_and_realign_visible_tree_indices() {
    let expression = "@modid (=omnibus_nested | =omnibus_leaf) & @loaders =Fabric";
    for mode in [vec![], vec!["--raw"], vec!["--no-ui"]] {
        let mut args = vec!["mod", "list", "complex-mod.jar"];
        args.extend(mode);
        args.extend(["--filter", expression, "-C", "M"]);
        assert_eq!(stdout(run(&args)), "omnibus_nested\n");
        args.push("--expanded");
        assert_eq!(stdout(run(&args)), "omnibus_nested\nomnibus_leaf\n");
    }
    let tree = stdout(run(&[
        "--raw",
        "mod",
        "list",
        "complex-mod.jar",
        "-F",
        "@modid =omnibus_leaf & @loaders =Fabric",
        "--index-type",
        "tree",
        "-C",
        "IM",
    ]));
    assert_eq!(tree, "[1][fabric]<1><1>\tomnibus_leaf\n");
    let plain = stdout(run(&["mod", "list", "complex-mod.jar"]));
    assert_eq!(
        stdout(run(&["mod", "list", "complex-mod.jar", "-F", " "])),
        plain
    );
}

#[test]
fn server_input_selection_is_independent_of_filter_and_all_can_be_combined_with_it() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::create_dir(root.path().join("mods")).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::copy(
        fixtures.join("servers/forge/.mcpm/server.json"),
        root.path().join(".mcpm/server.json"),
    )
    .unwrap();
    for name in ["first.jar", "second.jar"] {
        fs::copy(
            fixtures.join("complex-mod.jar"),
            root.path().join("mods").join(name),
        )
        .unwrap();
    }
    let server = root.path().to_str().unwrap();
    let base = [
        "--raw", "server", "mods", "list", "--server", server, "-C", "M",
    ];
    for (selection, expected) in [
        (vec!["--all"], "omnibus_leaf\nomnibus_leaf\n"),
        (vec!["--file", "first.jar"], "omnibus_leaf\n"),
        (
            vec!["--name", "Complex Mod / Forge"],
            "omnibus_leaf\nomnibus_leaf\n",
        ),
        (vec!["--modid", "omnibus"], "omnibus_leaf\nomnibus_leaf\n"),
        (vec!["--file", "second.jar"], "omnibus_leaf\n"),
    ] {
        let mut args = base.to_vec();
        args.extend(selection);
        args.extend(["-F", "@modid =omnibus_leaf"]);
        assert_eq!(stdout(run(&args)), expected);
    }
    let mut args = base.to_vec();
    args.extend(["--file", "first.jar", "-F", "@file second.jar"]);
    assert!(
        stdout(run(&args)).is_empty(),
        "Filter must not select new input files"
    );
}

#[test]
fn clap_rejects_invalid_filters_before_reading_inputs_and_help_describes_the_type() {
    let invalid = run(&["mod", "list", "missing.jar", "-F", "@loaders (Forge |)"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    let message = String::from_utf8(invalid.stderr).unwrap();
    assert!(message.contains("expected a matching value") && message.contains("column"));
    assert!(!message.contains("failed to read list input"));
    let help = stdout(run(&["server", "mods", "list", "--help"]));
    let inputs = help
        .split("Input options:\n")
        .nth(1)
        .unwrap()
        .split("\nFilter:\n")
        .next()
        .unwrap();
    for flag in ["--file", "--modid", "--name", "--all"] {
        assert!(inputs.contains(flag));
    }
    for text in [
        "Type: Filter",
        "-F, --filter <FILTER>",
        "A & B",
        "A | B",
        "@loaders (Forge | Fabric)",
        "\\(",
        "\\\"",
    ] {
        assert!(help.contains(text), "{text}");
    }
}
