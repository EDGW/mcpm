//! CLI cache-category selection and read-only Identify reuse independent of loader descriptors.

use kako_mods_lib::local::cache::hash::Blake3Hash;
use kako_mods_lib::local::cache::mods::ModsCacheDir;
use kako_mods_lib::source::FileHash;
use kako_mods_lib::source::FileId;
use kako_mods_lib::source::FileMatch;
use kako_mods_lib::source::FileRef;
use kako_mods_lib::source::FileSummary;
use kako_mods_lib::source::MatchedFileRole;
use kako_mods_lib::source::ProjectId;
use kako_mods_lib::source::ProjectRef;
use kako_mods_lib::source::SourceId;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn seed(cache: &ModsCacheDir, source: &Path) -> Blake3Hash {
    let writer = &cache;
    let id = writer
        .import(
            source,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
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
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .args(["--raw", "--log-level", "off"])
        .args(args)
        .arg("--cache-dir")
        .arg(cache)
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
            "mod",
            "cache",
            "mod.jar",
            "--metas",
            "ident",
            "--sources",
            "curseforge@PRIVATE-KEY",
        ],
    ));
    assert_eq!(record["metadata_requested"], false);
    assert_eq!(record["identification"].as_object().unwrap().len(), 1);
    assert_eq!(record["identification"]["curseforge"]["status"], "matched");
    assert!(!metadata.digest_path().exists());
    let identified = run(
        root.path(),
        cache.path(),
        &[
            "mod",
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
    // Defaults request both result categories and every built-in source.
    let record = first(&run(
        root.path(),
        cache.path(),
        &["mod", "cache", "mod.jar"],
    ));
    assert_eq!(record["status"], "available");
    assert_eq!(record["identification"].as_object().unwrap().len(), 2);
    fs::write(info.path(), b"broken source snapshot").unwrap();
    first(&run(
        root.path(),
        cache.path(),
        &["mod", "cache", "mod.jar", "--metas", "meta"],
    ));
    assert_eq!(fs::read(info.path()).unwrap(), b"broken source snapshot");
}

#[cfg(unix)]
#[test]
fn ssh_server_cache_enriches_on_the_client_and_uses_the_file_hosts_snapshots() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("host");
    let client = root.path().join("client");
    fs::create_dir_all(host.join("server/.mcpm")).unwrap();
    fs::create_dir_all(host.join("server/mods")).unwrap();
    fs::create_dir_all(&client).unwrap();
    fs::write(
        host.join("server/.mcpm/mcpm.json"),
        br#"{"version":1,"type":"local"}"#,
    )
    .unwrap();
    fs::write(
        host.join("server/.mcpm/server.json"),
        br#"{"version":2,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"1.0"}}"#,
    )
    .unwrap();
    let source = host.join("server/mods/mod.jar");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        &source,
    )
    .unwrap();
    let cache = ModsCacheDir::initialize(host.join("cache")).unwrap();
    let id = seed(&cache, &source);
    let ssh = root.path().join("ssh");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_quote(host.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
    let output = run(
        &client,
        Path::new("cache"),
        &[
            "--ssh",
            &connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm"),
            "server",
            "mods",
            "cache",
            "--server",
            "server",
            "--all",
        ],
    );
    let record = first(&output);
    assert_eq!(record["identification"].as_object().unwrap().len(), 2);
    assert_eq!(record["status"], "available");
    assert!(cache.mod_metadata(id).digest_path().exists());
    assert!(!client.join("cache").exists());
    let output = run(
        &client,
        Path::new("cache"),
        &[
            "--ssh",
            &connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm"),
            "server",
            "mods",
            "source",
            "identify",
            "--server",
            "server",
            "--all",
            "--source",
            "curseforge@PRIVATE-KEY",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("additional.jar"));
}

#[test]
fn remote_publication_acknowledges_verified_data_and_rejects_incorrect_content() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.jar");
    fs::write(&path, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let id = cache
        .import(
            &path,
            kako_mods_lib::local::cache::CacheLinkPreference::Copy,
        )
        .unwrap();
    let summary = FileSummary::read(&path, Default::default()).unwrap();
    let file = FileMatch {
        reference: FileRef {
            project: ProjectRef {
                source: SourceId("curseforge".into()),
                id: ProjectId("project".into()),
            },
            id: FileId("file".into()),
        },
        filename: "extra.jar".into(),
        size: summary.size,
        hashes: vec![FileHash::Sha1(summary.sha1)],
        role: MatchedFileRole::Additional,
    };
    let request = |file: &FileMatch| {
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm"))
            .args(["--log-level", "off", "remote"])
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(process.stdin.take().unwrap(), "{}", serde_json::json!({
            "guid":uuid::Uuid::from_u128(1), "command":"cache.mods.identification.write",
            "contents":{"cache":{"kind":"directory","path":"cache"}, "id":id.to_string(), "file":file}
        })).unwrap();
        let output = process.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>()
    };
    let replies = request(&file);
    assert!(
        replies
            .iter()
            .any(|reply| reply["status"] == "data" && reply["contents"].is_null())
    );
    assert!(replies.iter().any(|reply| reply["status"] == "finished"));
    let saved = fs::read(cache.ident_info(id).path()).unwrap();
    let mut wrong = file.clone();
    wrong.hashes = vec![FileHash::Sha1("0".repeat(40))];
    let replies = request(&wrong);
    assert!(replies.iter().any(|reply| reply["status"] == "error"));
    assert_eq!(fs::read(cache.ident_info(id).path()).unwrap(), saved);
    fs::remove_file(cache.mod_metadata(id).data_path()).unwrap();
    let replies = request(&file);
    assert!(replies.iter().any(|reply| reply["status"] == "error"));
}
