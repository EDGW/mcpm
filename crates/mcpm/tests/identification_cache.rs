//! CLI cache-category selection and read-only Identify reuse independent of loader descriptors.

use kako_mc_lib::source::FileHash;
use kako_mc_lib::source::FileId;
use kako_mc_lib::source::FileMatch;
use kako_mc_lib::source::FileRef;
use kako_mc_lib::source::FileSummary;
use kako_mc_lib::source::MatchedFileRole;
use kako_mc_lib::source::ProjectId;
use kako_mc_lib::source::ProjectRef;
use kako_mc_lib::source::SourceId;
use mcpm_backend::data::cache::hash::Blake3Hash;
use mcpm_backend::data::cache::mods::ModsCacheDir;
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
            mcpm_backend::data::cache::CacheLinkPreference::Copy,
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
    assert_eq!(record["identification"].as_object().unwrap().len(), 1);
    assert_eq!(record["identification"]["curseforge"]["status"], "matched");
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
    // Defaults request both result categories and every built-in source.
    let record = first(&run(
        root.path(),
        cache.path(),
        &["mods", "cache", "mod.jar"],
    ));
    assert_eq!(record["status"], "available");
    assert_eq!(record["identification"].as_object().unwrap().len(), 2);
    fs::write(info.path(), b"broken source snapshot").unwrap();
    first(&run(
        root.path(),
        cache.path(),
        &["mods", "cache", "mod.jar", "--metas", "meta"],
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
    let cache = ModsCacheDir::initialize(host.join("home/.mcpm/cache/mods")).unwrap();
    let id = seed(&cache, &source);
    let ssh = root.path().join("ssh");
    fs::write(
        &ssh,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nexport HOME={}\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
            shlex::try_quote(host.to_str().unwrap()).unwrap(),
            shlex::try_quote(host.join("home").to_str().unwrap()).unwrap()
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
            env!("CARGO_BIN_EXE_mcpm-tools"),
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
    assert!(!client.join("home/.mcpm/cache").exists());
    let output = run(
        &client,
        Path::new("cache"),
        &[
            "--ssh",
            &connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm-tools"),
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
        .import(&path, mcpm_backend::data::cache::CacheLinkPreference::Copy)
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
        let mut process = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"))
            .args(["--log-level", "off", "remote"])
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = process.stdin.take().unwrap();
        writeln!(input, "{}", serde_json::json!({
            "guid":uuid::Uuid::from_u128(1), "operation":"cache.mods.identification.write", "level":"off",
            "input":{"cache":{"kind":"directory","path":"cache"}, "id":id.to_string(), "file":file}
        })).unwrap();
        // Serve cancels accepted calls when its stdin closes, so keep it open until the call ends.
        let mut output = std::io::BufReader::new(process.stdout.take().unwrap());
        let mut replies = Vec::new();
        loop {
            let mut line = String::new();
            assert!(std::io::BufRead::read_line(&mut output, &mut line).unwrap() > 0);
            let reply: Value = serde_json::from_str(&line).unwrap();
            let terminal = reply["status"] == "finished" || reply["status"] == "failed";
            replies.push(reply);
            if terminal {
                break;
            }
        }
        drop(input);
        assert!(process.wait().unwrap().success());
        replies
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
    assert!(replies.iter().any(|reply| reply["status"] == "failed"));
    assert_eq!(fs::read(cache.ident_info(id).path()).unwrap(), saved);
    fs::remove_file(cache.mod_metadata(id).data_path()).unwrap();
    let replies = request(&file);
    assert!(replies.iter().any(|reply| reply["status"] == "failed"));
}
