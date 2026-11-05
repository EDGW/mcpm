//! CLI cache-category selection and read-only Identify reuse independent of loader descriptors.

use mcpm_lib::local::cache::hash::Blake3Hash;
use mcpm_lib::local::cache::mods::ModsCacheDir;
use mcpm_lib::source::FileHash;
use mcpm_lib::source::FileId;
use mcpm_lib::source::FileMatch;
use mcpm_lib::source::FileRef;
use mcpm_lib::source::FileSummary;
use mcpm_lib::source::MatchedFileRole;
use mcpm_lib::source::ProjectId;
use mcpm_lib::source::ProjectRef;
use mcpm_lib::source::SourceId;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn seed(cache: &ModsCacheDir, source: &Path) -> Blake3Hash {
    let writer = &cache;
    let id = writer
        .import(source, mcpm_lib::local::cache::CacheLinkPreference::Copy)
        .unwrap();
    let summary = FileSummary::read(source, Default::default()).unwrap();
    for key in ["modrinth", "curseforge"] {
        writer
            .write_ident_info(
                id,
                &FileMatch {
                    reference: FileRef {
                        project: ProjectRef {
                            source: SourceId(key.into()),
                            id: ProjectId("project".into()),
                        },
                        id: FileId("release".into()),
                    },
                    filename: "additional.jar".into(),
                    size: summary.size,
                    hashes: vec![FileHash::Sha1(summary.sha1.clone())],
                    role: MatchedFileRole::Additional,
                },
            )
            .unwrap();
    }
    id
}
fn run(root: &Path, cache: &Path, args: &[&str]) -> Output {
    fs::create_dir_all(root.join("home")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
    command
        .env("HOME", root.join("home"))
        .args(["--raw", "--log-level", "off"])
        .args(args);
    if !args.windows(2).any(|part| part == ["server", "mods"]) {
        command.arg("--cache-dir").arg(cache);
    }
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
fn first(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn category_and_source_selection_and_identify_leave_independent_snapshots_intact() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("mod.jar");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        &source,
    )
    .unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = seed(&cache, &source);
    let reader = &cache;
    let metadata = reader.mod_metadata(id);
    let info = reader.ident_info(id);
    let original = fs::read(info.path()).unwrap();
    let modified = fs::metadata(info.path()).unwrap().modified().unwrap();
    let record = first(&run(
        root.path(),
        cache.path(),
        &[
            "mods",
            "cache",
            "mod.jar",
            "--metas",
            "ident",
            "--sources",
            "curseforge@PRIVATE-KEY",
        ],
    ));
    assert_eq!(record["metadata_requested"], false);
    assert_eq!(
        record["identification"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        record["identification"]["sources"][0]["source"],
        "curseforge"
    );
    assert_eq!(
        record["identification"]["sources"][0]["resolution"]["kind"],
        "cached"
    );
    assert!(!metadata.digest_path().exists());
    let identified = run(
        root.path(),
        cache.path(),
        &[
            "mods",
            "source",
            "identify",
            "mod.jar",
            "--source",
            "curseforge@ANOTHER-KEY",
        ],
    );
    assert!(identified.status.success(), "{identified:?}");
    assert!(String::from_utf8_lossy(&identified.stdout).contains("additional.jar"));
    assert_eq!(fs::read(info.path()).unwrap(), original);
    assert_eq!(
        fs::metadata(info.path()).unwrap().modified().unwrap(),
        modified
    );
    assert!(!metadata.digest_path().exists());
    // Combining both cached providers with local metadata does not require a usable API key.
    let record = first(&run(
        root.path(),
        cache.path(),
        &[
            "mods",
            "cache",
            "mod.jar",
            "--metas",
            "meta,ident",
            "--sources",
            "modrinth,curseforge@",
        ],
    ));
    assert_eq!(record["status"], "available");
    assert_eq!(record["query_status"], "success");
    assert_eq!(record["cache_coverage"], "all");
    fs::write(info.path(), b"broken source snapshot").unwrap();
    first(&run(
        root.path(),
        cache.path(),
        &["mods", "cache", "mod.jar", "--metas", "meta"],
    ));
    assert_eq!(fs::read(info.path()).unwrap(), b"broken source snapshot");
}
