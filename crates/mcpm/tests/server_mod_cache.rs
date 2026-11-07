//! Server cache integration: scoped selection, declared loaders, read-only reuse and host routing.

use mcpm_lib::local::cache::mods::ModsCacheDir;
use mcpm_lib::local::cache::mods::meta::CachedIdentification;
use mcpm_lib::local::modloader::BUILTIN_LOADERS;
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpm-tools"));
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
    assert_eq!(records[0]["overall_status"], "success");
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
fn caching_reuses_objects_completes_loader_metadata_and_queries_remain_read_only() {
    let root = tempfile::tempdir().unwrap();
    server(&root.path().join("fabric"), "fabric");
    server(&root.path().join("forge"), "forge");
    let cache = ModsCacheDir::initialize(root.path().join("home/.mcpm/cache/mods")).unwrap();
    let first = run(
        root.path(),
        &["server", "mods", "cache", "--server", "fabric", "--all"],
        "",
    );
    assert!(first.status.success(), "{first:?}");
    let first = rows(&first);
    assert_eq!(first[0]["published"], true);
    let id = first[0]["id"].as_str().unwrap().parse().unwrap();
    let second = run(
        root.path(),
        &["server", "mods", "cache", "--server", "forge", "--all"],
        "",
    );
    assert!(second.status.success(), "{second:?}");
    assert_eq!(rows(&second)[0]["published"], false);
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
    for command in ["info", "list"] {
        let observed = run(
            root.path(),
            &["server", "mods", command, "--server", "fabric", "--all"],
            "",
        );
        assert!(observed.status.success(), "{observed:?}");
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
