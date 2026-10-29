//! Multi-backup inputs and continuation after verification or deletion failures.

use std::{
    fs,
    io::Write,
    num::NonZeroUsize,
    path::Path,
    process::{Command, Output, Stdio},
};

use mcpm_backend::local::{
    cache::{CacheLinkPreference, mods::ModsCacheDir},
    mcmod::backup::{self, CreateOptions, CreateStorage},
};

fn run(root: &Path, operation: &str, paths: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
        .args([
            "--raw",
            "--log-level",
            "off",
            "mods",
            "backup",
            operation,
            "--headers",
        ])
        .args(paths)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn arguments_and_literal_stdin_paths_verify_all_backups_after_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("file.jar"),
        b"not a zip; verification must not unpack this file",
    )
    .unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let valid = root.path().join("valid backup");
    let created = backup::create(
        &source,
        &valid,
        &mcpm_backend::local::runtime::RuntimeEnvironment::new().with_backup_cache(
            mcpm_backend::local::cache::mods::ModsCacheSelection::Directory {
                path: cache.path().to_owned(),
            },
        ),
        CreateOptions {
            storage: CreateStorage::Full(CacheLinkPreference::Copy),
            parallelism: NonZeroUsize::new(2).unwrap(),
        },
        |_| Ok(()),
    )
    .unwrap();
    for file in created.manifest().files() {
        cache.remove(&file.blake3()).unwrap();
    }
    let output = run(
        root.path(),
        "verify",
        &["missing-backup"],
        "\nvalid backup\nvalid backup\n",
    );
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<Vec<_>> = stdout
        .lines()
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], ["backup", "status", "files", "size", "checksum"]);
    assert_eq!(rows[1], ["missing-backup", "invalid", "", "", ""]);
    assert_eq!(rows[2][1], "valid");
    assert_eq!(rows[2][2], "1");
    assert_eq!(rows[2][4], created.manifest().checksum().to_string());
    assert_eq!(
        fs::read(valid.join("data/file.jar")).unwrap(),
        fs::read(source.join("file.jar")).unwrap()
    );
    let valid_output = run(root.path(), "verify", &[], "valid backup\n");
    assert!(valid_output.status.success());
    assert!(valid_output.stderr.is_empty());
    assert!(!run(root.path(), "verify", &[], "\n").status.success());
    backup::delete(&valid).unwrap();
}

#[test]
fn deletion_combines_arguments_and_stdin_and_continues_after_a_refused_directory() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file.jar"), b"cached content").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let options = CreateOptions {
        storage: CreateStorage::Full(CacheLinkPreference::HardLink),
        parallelism: NonZeroUsize::new(2).unwrap(),
    };
    for name in ["first backup", "second backup"] {
        backup::create(
            &source,
            &root.path().join(name),
            &mcpm_backend::local::runtime::RuntimeEnvironment::new().with_backup_cache(
                mcpm_backend::local::cache::mods::ModsCacheSelection::Directory {
                    path: cache.path().to_owned(),
                },
            ),
            options,
            |_| Ok(()),
        )
        .unwrap();
    }
    let output = run(
        root.path(),
        "delete",
        &["source", "first backup"],
        "\nfirst backup\nsecond backup\nsecond backup\n",
    );
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "backup\tstatus\nsource\tfailed\nfirst backup\tdeleted\nsecond backup\tdeleted\n"
    );
    assert_eq!(
        fs::read(source.join("file.jar")).unwrap(),
        b"cached content"
    );
    assert!(!root.path().join("first backup").exists());
    assert!(!root.path().join("second backup").exists());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("caused by:"));
    assert!(!run(root.path(), "delete", &[], "\n").status.success());

    backup::create(
        &source,
        &root.path().join("last backup"),
        &mcpm_backend::local::runtime::RuntimeEnvironment::new().with_backup_cache(
            mcpm_backend::local::cache::mods::ModsCacheSelection::Directory {
                path: cache.path().to_owned(),
            },
        ),
        options,
        |_| Ok(()),
    )
    .unwrap();
    let output = run(root.path(), "delete", &[], "last backup\nlast backup\n");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}
