//! Server cache integration: scoped selection, declared loaders, read-only reuse and host routing.

use mcpm_backend::local::cache::mods::ModsCacheDir;
use mcpm_backend::local::cache::mods::meta::CachedIdentification;
use mcpm_backend::local::modloader::BUILTIN_LOADERS;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn server(root: &Path, loader: &str) {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    fs::create_dir_all(root.join("mods")).unwrap();
    fs::write(
        root.join(".mcpm/mcpm.json"),
        br#"{"version":1,"type":"local"}"#,
    )
    .unwrap();
    fs::write(
        root.join(".mcpm/server.json"),
        serde_json::to_vec(&json!({
            "version":2,"minecraft_version":"1.21.1","loader":{"type":loader,"version":"1.0"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
        root.join("mods/example.jar.disabled"),
    )
    .unwrap();
}

fn run(root: &Path, args: &[&str], input: &str) -> Output {
    fs::create_dir_all(root.join("home")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm"));
    command
        .env("HOME", root.join("home"))
        .args(["--raw", "--log-level", "off"])
        .args(args);
    if args
        .windows(3)
        .any(|part| part == ["server", "mods", "cache"])
    {
        command.args(["--metas", "meta"]);
    }
    let mut child = command
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
fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn remote(root: &Path, command: &str, contents: Value) -> Vec<Value> {
    let output = run(
        root,
        &["remote"],
        &format!(
            "{}\n",
            json!({"guid":uuid::Uuid::from_u128(1), "command":command, "contents":contents})
        ),
    );
    assert!(output.status.success(), "{output:?}");
    let replies = rows(&output);
    assert_eq!(
        replies[replies.len() - 2]["status"],
        "finished",
        "{replies:?}"
    );
    replies
        .into_iter()
        .filter(|reply| reply["status"] == "data")
        .map(|reply| reply["contents"].clone())
        .collect()
}

#[test]
fn selectors_stay_in_server_scope_and_persist_whole_metadata_for_only_the_declared_loader() {
    let root = tempfile::tempdir().unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("home/.mcpm/cache/mods")).unwrap();
    server(&root.path().join("server"), "fabric");
    fs::copy(
        root.path().join("server/mods/example.jar.disabled"),
        root.path().join("outside.jar"),
    )
    .unwrap();
    let output = run(
        root.path(),
        &[
            "server",
            "mods",
            "cache",
            "--server",
            "server",
            "--modid",
            "omnibus_nested",
            "--file",
            "mods/example.jar.disabled",
            "--file",
            "../outside.jar",
            "--input",
            "name",
            "--parallelism",
            "3",
        ],
        "Complex Mod / Fabric\n",
    );
    assert!(!output.status.success(), "{output:?}");
    let records = rows(&output);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["success"], true);
    assert_eq!(records[0]["method"], "copy");
    let id = records[0]["id"].as_str().unwrap().parse().unwrap();
    let entry = cache.mod_metadata(id);
    let digest = entry.digest().unwrap();
    assert!(matches!(
        digest.identification("forge"),
        CachedIdentification::Missing
    ));
    let CachedIdentification::Recognized(meta) = digest.identification("fabric") else {
        panic!("expected fabric");
    };
    let meta = meta.to_mod_meta(&BUILTIN_LOADERS).unwrap();
    assert!(meta.mods.iter().any(|info| info.modid == "omnibus"));
    assert_eq!(meta.nested.len(), 3);
    assert!(!meta.nested[0].meta.nested.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside.jar"));
    assert_eq!(cache.list_metadata().unwrap().len(), 1);
}

#[test]
fn remote_cache_reuses_objects_completes_loader_metadata_and_queries_remain_read_only() {
    let root = tempfile::tempdir().unwrap();
    server(&root.path().join("fabric"), "fabric");
    server(&root.path().join("forge"), "forge");
    let cache = ModsCacheDir::initialize(root.path().join("home/.mcpm/cache/mods")).unwrap();
    let request = |server: &str| json!({"server":server, "selection":{"all":true}, "writing":{"parallelism":3,"metas":["meta"]}});
    let first = remote(root.path(), "server.mods.cache", request("fabric"));
    let file = &first.iter().find(|event| event["event"] == "file").unwrap()["contents"];
    assert_eq!(file["published"], true);
    let id = file["id"].as_str().unwrap().parse().unwrap();
    let second = remote(root.path(), "server.mods.cache", request("forge"));
    assert_eq!(second.last().unwrap()["contents"]["objects_reused"], 1);
    let entry = cache.mod_metadata(id);
    for loader in ["fabric", "forge"] {
        assert!(matches!(
            entry.digest().unwrap().identification(loader),
            CachedIdentification::Recognized(_)
        ));
    }
    assert!(matches!(
        entry.digest().unwrap().identification("neoforge"),
        CachedIdentification::Missing
    ));
    let before = fs::read(entry.digest_path()).unwrap();
    for command in ["server.mods.info", "server.mods.list"] {
        let events = remote(
            root.path(),
            command,
            json!({"server":"fabric", "selection":{"all":true}}),
        );
        assert_eq!(
            events.last().unwrap()["cache"],
            json!({"fully_cached":1,"partially_cached":0,"total":1})
        );
        assert_eq!(fs::read(entry.digest_path()).unwrap(), before);
    }
    // A corrupt Server cache must fail; disabling caching bypasses root validation.
    fs::write(
        cache.path().join(".mcpm/metadata.json"),
        b"invalid cache metadata",
    )
    .unwrap();
    for command in ["info", "list"] {
        let failed = run(
            root.path(),
            &["server", "mods", command, "--server", "fabric", "--all"],
            "",
        );
        assert!(!failed.status.success(), "{failed:?}");
        let direct = run(
            root.path(),
            &[
                "server",
                "mods",
                command,
                "--server",
                "fabric",
                "--all",
                "--no-cache",
            ],
            "",
        );
        assert!(direct.status.success(), "{direct:?}");
    }
}

#[cfg(unix)]
#[test]
fn ssh_and_saved_remote_use_source_home_and_ignore_client_server_and_cache() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("host");
    let client = root.path().join("client");
    server(&host.join("server"), "fabric");
    fs::create_dir_all(client.join("server/.mcpm")).unwrap();
    fs::write(
        client.join("server/.mcpm/server.json"),
        b"invalid client metadata",
    )
    .unwrap();
    let home = host.join("home");
    fs::create_dir_all(&home).unwrap();
    let ssh = root.path().join("ssh");
    fs::write(&ssh, format!("#!/bin/sh\ncd {} || exit 1\nexport HOME={}\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
        shlex::try_quote(host.to_str().unwrap()).unwrap(), shlex::try_quote(home.to_str().unwrap()).unwrap())).unwrap();
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    let connection = shlex::try_join([ssh.to_str().unwrap(), "test@host"]).unwrap();
    let first = run(
        &client,
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
        "",
    );
    assert!(first.status.success(), "{first:?}");
    assert_eq!(rows(&first)[0]["published"], true);
    let cache = ModsCacheDir::new(home.join(".mcpm/cache/mods")).unwrap();
    assert_eq!(cache.list_metadata().unwrap().len(), 1);
    assert!(!client.join("home/.mcpm/cache").exists());
    let workspace = client.join("workspace");
    fs::create_dir_all(workspace.join(".mcpm")).unwrap();
    fs::write(
        workspace.join(".mcpm/mcpm.json"),
        br#"{"version":1,"type":"remote"}"#,
    )
    .unwrap();
    fs::write(workspace.join(".mcpm/server.json"), serde_json::to_vec(&json!({"version":2,"ssh":connection,"ssh_mcpm":env!("CARGO_BIN_EXE_mcpm"),"path":"server"})).unwrap()).unwrap();
    let repeated = run(
        &client,
        &[
            "server",
            "mods",
            "cache",
            "--server",
            workspace.to_str().unwrap(),
            "--input",
            "modid",
        ],
        "omnibus_nested\n",
    );
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(rows(&repeated)[0]["published"], false);
    let partial = run(
        &client,
        &[
            "server",
            "mods",
            "cache",
            "--server",
            workspace.to_str().unwrap(),
            "--file",
            "missing.jar",
            "--file",
            "example.jar.disabled",
        ],
        "",
    );
    assert!(!partial.status.success(), "{partial:?}");
    let records = rows(&partial);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["failed_stage"], "import");
    assert_eq!(records[1]["success"], true);
    assert_eq!(records[1]["published"], false);
    assert!(String::from_utf8_lossy(&partial.stderr).contains("remote error:"));
    for command in ["info", "list"] {
        let result = run(
            &client,
            &[
                "server",
                "mods",
                command,
                "--server",
                workspace.to_str().unwrap(),
                "--all",
            ],
            "",
        );
        assert!(result.status.success(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("omnibus"));
    }
    assert!(!workspace.join("mods").exists());
    // Bypassing the host cache leaves its existing metadata intact.
    let entry = cache.list_metadata().unwrap().remove(0);
    let metadata = cache.mod_metadata(entry.id());
    let before = fs::read(metadata.digest_path()).unwrap();
    let explicit = run(
        &client,
        &[
            "--ssh",
            &connection,
            "--ssh-mcpm",
            env!("CARGO_BIN_EXE_mcpm"),
            "server",
            "mods",
            "info",
            "--server",
            "server",
            "--all",
            "--no-cache",
        ],
        "",
    );
    assert!(explicit.status.success(), "{explicit:?}");
    assert_eq!(fs::read(metadata.digest_path()).unwrap(), before);
}
