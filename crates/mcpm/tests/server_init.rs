//! CLI server identification and initialization using declarations rather than launch validation.

use mcpm_lib::local::{
    mc::server::{MinecraftServer, identify},
    modloader::BUILTIN_LOADERS,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/server-init")
        .join(name)
}
fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        // Fixtures may also be initialized manually; start each test unregistered.
        if entry.file_name() == ".mcpm" {
            continue;
        }
        let path = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &path);
        } else {
            fs::copy(entry.path(), path).unwrap();
        }
    }
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpm"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn init_identifies_loaders_and_publishes_configuration_without_legacy_command() {
    for (case, loader, mc, version) in [
        ("forge", Some("forge"), "1.20.1", "47.3.12"),
        ("neoforge", Some("neoforge"), "1.21.1", "21.1.77"),
        ("early-neoforge", Some("neoforge"), "1.20.1", "47.1.79"),
        ("forge-shim", Some("forge"), "1.21.1", "52.0.1"),
        ("fabric", Some("fabric"), "1.20.1", "0.16.10"),
        ("fabric-auto", Some("fabric"), "1.20.1", "0.16.10"),
        ("vanilla", None, "1.20.1", ""),
        ("fake-vanilla", None, "1.20.1", ""),
        ("partial-unknown", None, "1.20.1", ""),
        ("bundled-vanilla", None, "1.21.1", ""),
        ("nested-batch", None, "1.20.1", ""),
    ] {
        let temp = TempDir::new().unwrap();
        copy(&fixture(case), temp.path());
        let script = if case == "nested-batch" {
            "run.bat"
        } else {
            "run.sh"
        };
        let output = run(temp.path(), &["init", "--start-script", script]);
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let server = MinecraftServer::load(temp.path()).unwrap();
        assert_eq!(server.metadata().minecraft_version, mc, "{case}");
        assert_eq!(
            server
                .metadata()
                .loader
                .declaration()
                .map(|l| l.loader_type.as_str()),
            loader,
            "{case}"
        );
        if let Some(info) = server.resolved_loader() {
            assert_eq!(info.version, version);
        }
        let launch = mcpm_lib::local::mc::server::config::decode_launch(
            &fs::read_to_string(temp.path().join("launch.toml")).unwrap(),
        )
        .unwrap();
        assert!(
            launch
                .jvm
                .extra_arguments
                .iter()
                .all(|arg| !arg.starts_with('@'))
        );
        mcpm_lib::local::mc::server::config::decode_conf(
            &fs::read_to_string(temp.path().join("conf.toml")).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn init_preserves_unknown_jvm_options_from_start_script() {
    let temp = TempDir::new().unwrap();
    copy(&fixture("unknown-jvm-options"), temp.path());
    let output = run(temp.path(), &["init", "--start-script", "run.sh"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let server = MinecraftServer::load(temp.path()).unwrap();
    assert_eq!(server.metadata().minecraft_version, "1.20.1");
    let launch = mcpm_lib::local::mc::server::config::decode_launch(
        &fs::read_to_string(temp.path().join("launch.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        launch.jvm.extra_arguments,
        [
            "--unknown-launcher-option",
            "--future-option=value",
            "-XX:FutureTuning=42",
        ]
    );
    mcpm_lib::local::mc::server::config::decode_conf(
        &fs::read_to_string(temp.path().join("conf.toml")).unwrap(),
    )
    .unwrap();
}

#[test]
fn fml_uses_command_evidence_without_opening_dependency_jars() {
    for (case, mc, version) in [
        ("forge", "1.20.1", "47.3.12"),
        ("neoforge", "1.21.1", "21.1.77"),
        ("early-neoforge", "1.20.1", "47.1.79"),
        ("forge-shim", "1.21.1", "52.0.1"),
    ] {
        let temp = TempDir::new().unwrap();
        copy(&fixture(case), temp.path());
        fs::remove_dir_all(temp.path().join("libraries")).unwrap();
        for script in ["run.sh", "run.bat"] {
            if !temp.path().join(script).is_file() {
                continue;
            }
            let path = temp.path().join(script);
            let original = fs::read_to_string(&path).unwrap();
            let helper = if script.ends_with(".bat") {
                "helper.exe %UNRESOLVED_HELPER%\r\njava -jar absent-helper.jar\r\n"
            } else {
                "helper \"$UNRESOLVED_HELPER\"\njava -jar absent-helper.jar\n"
            };
            fs::write(&path, format!("{helper}{original}")).unwrap();
            let candidates = identify::identify(temp.path(), script, &BUILTIN_LOADERS).unwrap();
            assert_eq!(
                candidates[0].metadata.minecraft_version, mc,
                "{case}: {script}"
            );
            assert_eq!(
                candidates[0].metadata.loader.declaration().unwrap().version,
                version,
                "{case}: {script}"
            );
        }
    }
}

#[test]
fn fabric_uses_minimum_version_evidence_and_falls_back_only_when_needed() {
    for case in ["fabric", "fabric-version-conflict", "fabric-auto"] {
        let temp = TempDir::new().unwrap();
        copy(&fixture(case), temp.path());
        let libraries = temp.path().join("libraries");
        if libraries.exists() {
            fs::remove_dir_all(libraries).unwrap();
            // The intermediary coordinate already supplies the game version.
            fs::remove_file(temp.path().join("server.jar")).unwrap();
        }
        let candidates = identify::identify(temp.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
        assert_eq!(candidates[0].metadata.minecraft_version, "1.20.1", "{case}");
        assert_eq!(
            candidates[0].metadata.loader.declaration().unwrap().version,
            "0.16.10",
            "{case}"
        );
    }
    // A direct KnotServer command has identity evidence, but still needs versions.
    let temp = TempDir::new().unwrap();
    copy(&fixture("fabric"), temp.path());
    fs::write(temp.path().join("run.sh"), "java -cp libraries/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar net.fabricmc.loader.impl.launch.knot.KnotServer\n").unwrap();
    fs::remove_dir_all(temp.path().join("libraries")).unwrap();
    let candidates = identify::identify(temp.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    assert_eq!(candidates[0].metadata.minecraft_version, "1.20.1");
    fs::remove_file(temp.path().join("server.jar")).unwrap();
    assert!(identify::identify(temp.path(), "run.sh", &BUILTIN_LOADERS).is_err());
}

#[test]
fn invalid_fixtures_fail_without_writing_and_force_preserves_previous_configuration() {
    for (case, script, reason) in [
        ("undefined-variable", "run.sh", "unresolved shell variable"),
        ("unquoted-variable", "run.sh", "word splitting"),
        ("cycle", "run.sh", "recursive"),
        ("argument-cycle", "run.sh", "cannot accurately identify"),
        ("unknown-main", "run.sh", "cannot accurately identify"),
        ("missing-jar", "run.sh", "failed to open"),
        ("corrupt-jar", "run.sh", "invalid JAR"),
        ("no-java", "run.sh", "no Java server launch"),
        ("delayed-batch", "run.bat", "delayed batch expansion"),
        ("duplicate-version", "run.sh", "conflicting game option"),
        (
            "fabric-auto-missing-version",
            "run.sh",
            "Fabric loader version is missing",
        ),
    ] {
        let temp = TempDir::new().unwrap();
        copy(&fixture(case), temp.path());
        let output = run(temp.path(), &["init", "--start-script", script]);
        assert_eq!(output.status.code(), Some(1), "{case}");
        assert!(output.stdout.is_empty(), "{case}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(reason),
            "{case}: expected {reason:?}, got {error}"
        );
        assert!(!temp.path().join(".mcpm").exists(), "{case}");
        let metadata_dir = temp.path().join(".mcpm");
        fs::create_dir(&metadata_dir).unwrap();
        let old = b"existing configuration, even invalid JSON, must survive failure\n";
        fs::write(metadata_dir.join("server.json"), old).unwrap();
        let output = run(temp.path(), &["init", "--force", "--start-script", script]);
        assert!(!output.status.success(), "{case}");
        assert_eq!(
            fs::read(metadata_dir.join("server.json")).unwrap(),
            old,
            "{case}"
        );
    }
}

#[test]
fn relaxed_recognition_keeps_known_state_when_skipping_script_statements() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    copy(&fixture("bundled-vanilla"), root);
    fs::write(
        root.join("run.sh"),
        "JAR=server.jar\nJAR=$UNDEFINED\nif true; then\n\
         cd somewhere-else\njava -jar not-a-candidate.jar\nfi\njava -jar \"$JAR\"\n",
    )
    .unwrap();

    let strict = run(root, &["--raw", "init", "-G", "--start-script", "run.sh"]);
    let error = String::from_utf8_lossy(&strict.stderr);
    assert_eq!(strict.status.code(), Some(1), "{error}");
    assert!(error.contains("unresolved shell variable JAR"), "{error}");
    assert!(error.contains("--ignore-unknown-vars"), "{error}");
    assert!(!root.join("launch.toml").exists());

    let relaxed = run(root, &["--raw", "init", "-IG", "--start-script", "run.sh"]);
    let warnings = String::from_utf8_lossy(&relaxed.stderr);
    assert!(relaxed.status.success(), "{warnings}");
    assert_eq!(warnings.matches("warning:").count(), 2, "{warnings}");
    assert!(warnings.contains("run.sh:2") && warnings.contains("run.sh:3"));
    let server = MinecraftServer::load(root).unwrap();
    assert_eq!(server.metadata().minecraft_version, "1.21.1");
}

#[test]
fn multiple_candidates_require_a_terminal_or_complete_manual_identity() {
    let temp = TempDir::new().unwrap();
    copy(&fixture("multiple"), temp.path());
    for args in [
        vec!["init", "--start-script", "run.sh"],
        vec!["--raw", "init", "--start-script", "run.sh"],
    ] {
        let output = run(temp.path(), &args);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("selection requires an interactive terminal"));
        assert!(error.contains("run.sh:1") && error.contains("run.sh:2"));
        assert!(!error.contains("--start-line"));
        assert!(!temp.path().join(".mcpm").exists());
    }
    for args in [
        vec!["init", "--minecraft-version", "1.21.1"],
        vec!["init", "--minecraft-version", "1.21.1", "--loader", "forge"],
        vec!["init", "--start-script", "run.sh", "--start-line", "2"],
    ] {
        let output = run(temp.path(), &args);
        assert!(!output.status.success());
        assert!(!temp.path().join(".mcpm").exists());
    }

    let output = run(
        temp.path(),
        &[
            "init",
            "--minecraft-version",
            "1.21.1",
            "--loader",
            "vanilla",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let server = MinecraftServer::load(temp.path()).unwrap();
    assert_eq!(server.metadata().minecraft_version, "1.21.1");
    assert!(!temp.path().join("launch.toml").exists());
}

#[test]
fn manual_init_publishes_ordered_arguments_and_typed_conf_only_when_authorized() {
    use mcpm_lib::local::mc::server::config::{JavaEntry, decode_conf, decode_launch};

    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let launch = root.join("launch.toml");
    let conf = root.join("conf.toml");
    let prior = "[java]\n[server]\nworld='old'\n";
    fs::write(&conf, prior).unwrap();
    let args = [
        "--raw",
        "init",
        "--no-git",
        "--minecraft-version",
        "1.21.1",
        "--loader",
        "vanilla",
        "--entry",
        "module@example.server:example.Main",
        "--launch-cwd",
        "a directory",
        "--launch-env",
        "VALUE=a=b",
        "--java-arg=-Done=1",
        "--java-args=-Dtwo='two words' -Dthree=3",
        "--java-arg=-Dfour=4",
        "--app-args=first 'second item'",
        "--app-arg=third item",
        "--app-args=fourth",
        "--conf",
        "p=25566",
        "--conf",
        "world=123",
        "--conf",
        "jvm.heap.maximum=4G",
        "--conf",
        "E=-Ddiscard=1",
        "--conf",
        "extra:=",
        "--conf",
        "E+=-Dfinal=1",
    ];
    let rejected = run(root, &args);
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        !rejected.status.success() && error.contains("--force --foverride-conf"),
        "{error}"
    );
    assert!(!root.join(".mcpm/server.json").exists() && !launch.exists());
    assert_eq!(fs::read_to_string(&conf).unwrap(), prior);

    let initialized = [&args[..], &["-F"]].concat();
    let output = run(root, &initialized);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let launch = decode_launch(&fs::read_to_string(&launch).unwrap()).unwrap();
    assert_eq!(
        launch.entry,
        JavaEntry::Module {
            module: "example.server".into(),
            main_class: Some("example.Main".into())
        }
    );
    assert_eq!(
        launch.process.working_directory.as_deref(),
        Some("a directory")
    );
    assert_eq!(launch.process.environment["VALUE"], "a=b");
    assert_eq!(
        launch.jvm.extra_arguments,
        ["-Done=1", "-Dtwo=two words", "-Dthree=3", "-Dfour=4"]
    );
    assert_eq!(
        launch.application.extra_arguments,
        ["first", "second item", "third item", "fourth"]
    );
    let settings = decode_conf(&fs::read_to_string(&conf).unwrap()).unwrap();
    assert_eq!(settings.server.port, Some(25566));
    assert_eq!(settings.server.world.as_deref(), Some("123"));
    assert_eq!(settings.jvm.heap.maximum.as_deref(), Some("4G"));
    assert_eq!(settings.jvm.extra_arguments, ["-Dfinal=1"]);

    let repeated = [&initialized[..], &["--conf", "server.port=25567"]].concat();
    let output = run(root, &repeated);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success()
            && error.contains("configuration key overridden more than once: server.port"),
        "{error}"
    );
}

#[test]
fn explicit_git_creates_nested_repository_and_preserves_existing_repository_or_worktree() {
    let parent = TempDir::new().unwrap();
    let git = |root: &Path, args: &[&str]| {
        let output = Command::new("git")
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    git(parent.path(), &["init", "--quiet"]);
    let parent_config = fs::read(parent.path().join(".git/config")).unwrap();
    let child = parent.path().join("server");
    fs::create_dir(&child).unwrap();
    let manual = [
        "init",
        "--minecraft-version",
        "1.21.1",
        "--loader",
        "vanilla",
    ];
    assert!(run(&child, &manual).status.success());
    assert!(!child.join(".git").exists());
    let mut explicit = manual.to_vec();
    explicit.extend(["--force", "--git"]);
    let output = run(&child, &explicit);
    assert!(output.status.success(), "{output:?}");
    assert!(child.join(".git").is_dir());
    git(&child, &["config", "mcpm.test", "preserve"]);
    let child_config = fs::read(child.join(".git/config")).unwrap();
    assert!(run(&child, &explicit).status.success());
    assert_eq!(fs::read(child.join(".git/config")).unwrap(), child_config);
    assert_eq!(
        fs::read(parent.path().join(".git/config")).unwrap(),
        parent_config
    );

    let hooks = parent.path().join("empty-hooks");
    fs::create_dir(&hooks).unwrap();
    git(
        &child,
        &[
            "-c",
            "user.name=mcpm test",
            "-c",
            "user.email=mcpm@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            &format!("core.hooksPath={}", hooks.display()),
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    let worktree = parent.path().join("worktree");
    git(
        &child,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let marker = fs::read(worktree.join(".git")).unwrap();
    let mut create = manual.to_vec();
    create.push("--git");
    let output = run(&worktree, &create);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(worktree.join(".git")).unwrap(), marker);
}
