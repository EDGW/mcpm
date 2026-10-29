//! CLI server identification and initialization using declarations rather than launch validation.

use mcpm_backend::local::{
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
fn init_identifies_loaders_and_retains_flattened_command() {
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
        ("unknown-jvm-options", None, "1.20.1", ""),
        ("bundled-vanilla", None, "1.21.1", ""),
        ("nested-shell", None, "1.20.1", ""),
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
                .as_ref()
                .map(|l| l.loader_type.as_str()),
            loader,
            "{case}"
        );
        if let Some(info) = server.loader() {
            assert_eq!(info.version, version);
        }
        let start = server.metadata().start.as_ref().unwrap();
        assert!(start.script.is_relative());
        assert!(start.arguments.iter().all(|arg| !arg.starts_with('@')));
    }
}

#[test]
fn fml_uses_command_evidence_without_opening_dependency_jars() {
    for (case, mc, version) in [
        ("forge", "1.20.1", "47.3.12"),
        ("neoforge", "1.21.1", "21.1.77"),
        ("early-neoforge", "1.20.1", "47.1.79"),
        ("forge-shim", "1.21.1", "52.0.1"),
        ("loader-version-conflict", "1.20.1", "47.4.0"),
        ("game-version-conflict", "1.20.1", "47.3.12"),
        ("missing-loader", "1.20.1", "47.3.12"),
        ("fake-loader", "1.20.1", "47.3.12"),
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
            // Conflict fixtures change only the shell argument file.
            let expected_version = if case == "loader-version-conflict" && script == "run.bat" {
                "47.3.12"
            } else {
                version
            };
            assert_eq!(
                candidates[0].metadata.minecraft_version, mc,
                "{case}: {script}"
            );
            assert_eq!(
                candidates[0].metadata.loader.as_ref().unwrap().version,
                expected_version,
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
            candidates[0].metadata.loader.as_ref().unwrap().version,
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
        ("dynamic-control", "run.sh", "control flow are unsupported"),
        ("dynamic-command", "run.sh", "unsupported shell expansion"),
        ("undefined-variable", "run.sh", "unresolved shell variable"),
        ("unquoted-variable", "run.sh", "word splitting"),
        ("cycle", "run.sh", "recursive"),
        ("argument-cycle", "run.sh", "nested Java"),
        ("unknown-main", "run.sh", "cannot accurately identify"),
        ("missing-jar", "run.sh", "failed to open"),
        ("corrupt-jar", "run.sh", "invalid JAR"),
        ("no-java", "run.sh", "no Java server launch"),
        ("dynamic-batch", "run.bat", "control flow are unsupported"),
        ("delayed-batch", "run.bat", "delayed batch expansion"),
        ("duplicate-version", "run.sh", "conflicting game option"),
        ("client-target", "run.sh", "not a production server"),
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
            error.contains("server remains unrecognized") && error.contains("initialize manually"),
            "{case}: {error}"
        );
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
fn multiple_candidates_require_a_terminal_and_partial_values_only_filter_identified_results() {
    let temp = TempDir::new().unwrap();
    copy(&fixture("multiple"), temp.path());
    let candidates = identify::identify(temp.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    assert_eq!(candidates.len(), 2);
    for args in [
        vec!["init", "--start-script", "run.sh"],
        vec!["--raw", "init", "--start-script", "run.sh"],
    ] {
        let output = run(temp.path(), &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("multiple identified server launches")
        );
        assert!(!temp.path().join(".mcpm").exists());
    }
    let output = run(temp.path(), &["init", "--minecraft-version", "1.21.1"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let server = MinecraftServer::load(temp.path()).unwrap();
    assert_eq!(server.metadata().minecraft_version, "1.21.1");
    assert!(
        server
            .metadata()
            .start
            .as_ref()
            .unwrap()
            .arguments
            .contains(&"other.jar".into())
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
