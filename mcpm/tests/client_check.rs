use std::{
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str]) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/client-check");
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .current_dir(&root)
        .args(["client", "check"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn check_accepts_multiple_ids_deduplicates_and_continues_after_failure() {
    let output = run(&["good", "corrupt", "good", "unverified"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.contains("\tsummary\t"))
            .count(),
        3
    );
    assert!(stdout.contains("good\tintact\tsummary\t\tscope=libraries:hash,assets:size,asset_index:hash,version_jar:hash,logging:hash, files=4"));
    assert!(stdout.contains("corrupt\tfailed\tsummary"));
    assert!(stdout.contains("SHA-1 mismatch"));
    assert!(stdout.contains("unverified\tunverified\tsummary"));
    assert!(stdout.contains("no SHA-1 metadata"));
    assert!(stdout.lines().all(|line| line.split('\t').count() == 5));
}

#[test]
fn check_scopes_directory_and_raw_output_are_supported() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/client-check");
    for (args, files) in [
        (
            vec![
                "good",
                "--assets",
                "skip",
                "--asset-index",
                "skip",
                "--version-jar",
                "skip",
            ],
            1,
        ),
        (
            vec![
                "good",
                "--libraries",
                "skip",
                "--version-jar",
                "skip",
                "--assets",
                "hash",
            ],
            2,
        ),
        (vec!["good", "--libraries", "size", "--assets", "hash"], 4),
        (
            vec!["good", "--raw", "--directory", root.to_str().unwrap()],
            4,
        ),
    ] {
        let output = run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("files={files}, verified={files}"))
        );
    }
}

#[test]
fn check_all_includes_broken_versions_and_never_prompts() {
    let output = run(&["--all", "--raw"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .lines()
            .filter(|line| line.contains("\tsummary\t"))
            .count(),
        4
    );
    assert!(stdout.contains("broken\tfailed\tsummary"));
    assert!(stdout.contains("good\tintact\tsummary"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("3 client version(s)"));
    assert_eq!(run(&["--all", "good"]).status.code(), Some(2));
}

#[test]
fn check_noninteractive_selection_and_invalid_ids_fail_with_actionable_errors() {
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("specify version IDs or use --all"));
    assert_eq!(run(&["--raw"]).status.code(), Some(1));
    let missing = run(&["not-installed", "good"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("good\tintact\tsummary"));
    let invalid = run(&["../invalid", "good"]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("good\tintact\tsummary"));
}
