use std::{ffi::OsString, fs, io::Write, path::Path};

use kako_mods_lib::local::mc::client::{
    ClientPlatform, MinecraftDirectory, RuleEnvironment,
    launch::{
        Identity, JavaVersionCheck, LaunchOptions, LaunchPlanError, LoggingOptions,
        PreparationError,
    },
};
use serde_json::{Value, json};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn profile(root: &Path, name: &str, value: Value) {
    write(
        root,
        &format!("versions/{name}/{name}.json"),
        value.to_string().as_bytes(),
    );
}
fn base() -> Value {
    json!({"id":"base", "mainClass":"example.Main", "javaVersion":{"majorVersion":17},
        "arguments":{"jvm":["-Djava.library.path=${natives_directory}","-cp","${classpath}"],
        "game":["--username","${auth_player_name}","--gameDir","${game_directory}",
            "--accessToken","${auth_access_token}"]}})
}
fn options() -> LaunchOptions {
    let os = match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    };
    let mut options = LaunchOptions::new(
        "java",
        Identity::offline("Player One"),
        RuleEnvironment::new(ClientPlatform {
            os: os.into(),
            arch: std::env::consts::ARCH.into(),
            version: "1".into(),
            bits: usize::BITS as u8,
        }),
    );
    options.java.version_check = JavaVersionCheck::Skip;
    options.logging = LoggingOptions::Disabled;
    options
}
fn root() -> TempDir {
    tempfile::Builder::new()
        .prefix("mcpm launch ")
        .tempdir()
        .unwrap()
}
fn archive(root: &Path, relative: &str, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    write(root, relative, &zip.finish().unwrap().into_inner());
}
fn native_library() -> Value {
    json!({"name":"example:natives:1", "natives":{"linux":"natives-test", "osx":"natives-test", "windows":"natives-test"},
        "downloads":{"classifiers":{"natives-test":{"path":"native.zip"}}}, "extract":{"exclude":["META-INF/"]}})
}

#[test]
fn launch_plan_preserves_inheritance_patch_priority_rules_and_argument_boundaries() {
    let root = root();
    let mut parent = base();
    parent["libraries"] = json!([
        {"name":"example:lib:1", "downloads":{"artifact":{"path":"old.jar"}}},
        {"name":"example:other:1", "downloads":{"artifact":{"path":"other.jar"}}},
        native_library()
    ]);
    parent["arguments"]["game"].as_array_mut().unwrap().push(
        json!({"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],
        "value":["--width","${resolution_width}","--height","${resolution_height}"]}),
    );
    profile(root.path(), "base", parent);
    profile(
        root.path(),
        "child",
        json!({"id":"child", "inheritsFrom":"base", "patches":[
            {"id":"high", "priority":20, "mainClass":"example.Loader", "arguments":{"jvm":["-Dpatch=high"],"game":["--patch","high"]},
             "libraries":[{"name":"example:lib:2","downloads":{"artifact":{"path":"new.jar"}}}]},
            {"id":"low", "priority":10, "arguments":{"jvm":["-Dpatch=low"]}}
        ]}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("child").unwrap();
    let mut options = options();
    options.identity = Identity::online(
        "Player One",
        "b50ad385-829d-3141-a216-7e7d7539ba7f",
        "secret-token",
    );
    options.directories.game = Some("game with spaces".into());
    options.memory.maximum_mib = Some(4096);
    options.window.width = Some(1280);
    options.window.height = Some(720);
    options
        .arguments
        .jvm
        .append
        .push("-Dlast=one argument".into());
    let plan = directory.plan_launch(&version, &options).unwrap();
    assert!(!plan.game_directory().exists()); // planning has no filesystem side effects
    assert!(!plan.natives_directory().exists());
    assert_eq!(plan.main_class(), "example.Loader");
    assert_eq!(
        plan.classpath()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["other.jar", "new.jar", "base.jar"]
    );
    assert_eq!(plan.native_archives().len(), 1);
    let jvm = plan.jvm_arguments();
    assert!(
        jvm.iter().position(|p| p == "-Dpatch=low") < jvm.iter().position(|p| p == "-Dpatch=high")
    );
    assert!(jvm.iter().any(|p| p == "-Xmx4096M"));
    assert_eq!(jvm.last().unwrap(), "-Dlast=one argument");
    let game = plan.game_arguments();
    assert_eq!(game[1], "Player One");
    assert_eq!(game[3], plan.game_directory().as_os_str());
    assert_eq!(game.iter().filter(|p| *p == "--width").count(), 1);
    assert!(game.iter().any(|p| p == "secret-token"));
    assert!(!format!("{plan:?} {:?}", options.identity).contains("secret-token"));
    assert_eq!(
        Identity::offline("Notch").uuid,
        "b50ad385829d3141a2167e7d7539ba7f"
    );
}

#[test]
fn hmcl_root_snapshots_are_not_appended_to_their_authoritative_patches() {
    let root = root();
    let mut aggregate = base();
    let mut game = aggregate.clone();
    game["id"] = "game".into();
    game["priority"] = 0.into();
    aggregate["root"] = true.into();
    aggregate["patches"] = json!([game]);
    profile(root.path(), "hmcl", aggregate);
    profile(
        root.path(),
        "child",
        json!({"id":"child", "inheritsFrom":"hmcl", "mainClass":"example.Child",
        "arguments":{"game":["--child","yes"],"jvm":["-Dchild=1"]}}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("child").unwrap();
    let plan = directory.plan_launch(&version, &options()).unwrap();
    assert_eq!(plan.main_class(), "example.Child");
    assert_eq!(
        plan.jvm_arguments().iter().filter(|p| *p == "-cp").count(),
        1
    );
    assert_eq!(
        plan.game_arguments()
            .iter()
            .filter(|p| *p == "--username")
            .count(),
        1
    );
    assert_eq!(
        &plan.game_arguments()[6..],
        ["--child", "yes"].map(OsString::from)
    );
    assert_eq!(plan.jvm_arguments().last().unwrap(), "-Dchild=1");
    assert!(
        plan.classpath()
            .last()
            .unwrap()
            .ends_with("versions/hmcl/hmcl.jar")
    );
}

#[test]
fn preparation_extracts_persistent_natives_maps_assets_and_rejects_unsafe_archives() {
    let root = root();
    let mut metadata = base();
    metadata["libraries"] = json!([native_library()]);
    metadata["assets"] = "test".into();
    profile(root.path(), "base", metadata);
    write(root.path(), "versions/base/base.jar", b"jar");
    write(root.path(), "assets/indexes/test.json", br#"{"virtual":true,"map_to_resources":true,"objects":{"sounds/test":{"hash":"a9993e364706816aba3e25717850c26c9cd0d89d","size":3}}}"#);
    write(
        root.path(),
        "assets/objects/a9/a9993e364706816aba3e25717850c26c9cd0d89d",
        b"abc",
    );
    archive(
        root.path(),
        "libraries/native.zip",
        &[
            ("META-INF/ignored", b"ignore"),
            ("sub/native.bin", b"native"),
        ],
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("base").unwrap();
    let plan = directory.plan_launch(&version, &options()).unwrap();
    let natives = plan.natives_directory().to_owned();
    assert_eq!(
        natives,
        root.path()
            .canonicalize()
            .unwrap()
            .join("versions/base/base-natives")
    );
    let prepared = plan.prepare().unwrap();
    assert_eq!(fs::read(natives.join("sub/native.bin")).unwrap(), b"native");
    assert!(!natives.join("META-INF").exists());
    assert_eq!(
        fs::read(root.path().join("assets/virtual/test/sounds/test")).unwrap(),
        b"abc"
    );
    assert_eq!(
        fs::read(root.path().join("resources/sounds/test")).unwrap(),
        b"abc"
    );
    drop(prepared);
    assert!(natives.exists());

    archive(
        root.path(),
        "libraries/native.zip",
        &[("../escape", b"escape")],
    );
    let plan = directory.plan_launch(&version, &options()).unwrap();
    let natives = plan.natives_directory().to_owned();
    assert!(matches!(
        plan.prepare(),
        Err(PreparationError::UnsafeEntry { .. })
    ));
    assert!(natives.exists());
    assert!(!natives.parent().unwrap().join("escape").exists());

    archive(root.path(), "libraries/native.zip", &[("native", b"12345")]);
    let mut options = options();
    options.natives.maximum_bytes = 4;
    assert!(matches!(
        directory.plan_launch(&version, &options).unwrap().prepare(),
        Err(PreparationError::ExtractionLimit { .. })
    ));
}

#[test]
fn legacy_arguments_and_explicit_replacements_use_the_same_placeholder_rules() {
    let root = root();
    profile(
        root.path(),
        "legacy",
        json!({"id":"legacy","mainClass":"example.Main",
        "minecraftArguments":"--username ${auth_player_name} --quoted 'one two' --custom ${custom}"}),
    );
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("legacy").unwrap();
    let mut options = options();
    assert!(matches!(
        directory.plan_launch(&version, &options),
        Err(LaunchPlanError::Argument(_))
    ));
    options
        .arguments
        .variables
        .insert("custom".into(), "value with spaces".into());
    let plan = directory.plan_launch(&version, &options).unwrap();
    assert_eq!(
        plan.game_arguments(),
        [
            "--username",
            "Player One",
            "--quoted",
            "one two",
            "--custom",
            "value with spaces"
        ]
        .map(OsString::from)
    );
    assert!(plan.jvm_arguments().iter().any(|p| p == "-cp"));
    options.arguments.game.replace = Some(vec!["--replacement".into(), "${custom}".into()]);
    assert_eq!(
        directory
            .plan_launch(&version, &options)
            .unwrap()
            .game_arguments(),
        ["--replacement", "value with spaces"].map(OsString::from)
    );
    options
        .arguments
        .variables
        .insert("auth_access_token".into(), "override".into());
    assert!(matches!(
        directory.plan_launch(&version, &options),
        Err(LaunchPlanError::Argument(_))
    ));
}

#[cfg(unix)]
#[test]
fn launch_process_receives_exact_argv_environment_and_cwd_and_retains_natives() {
    use kako_mods_lib::local::mc::client::launch::ProcessStream;
    use std::os::unix::fs::PermissionsExt;
    let root = root();
    profile(root.path(), "base", base());
    write(root.path(), "versions/base/base.jar", b"jar");
    let java = root.path().join("fake java");
    fs::write(&java, b"#!/bin/sh\nif [ \"$1\" = '-version' ]; then printf 'openjdk version \"17.0.1\"\\n' >&2; exit 0; fi\nprintf 'cwd=%s\\n' \"$PWD\"\nprintf 'env=%s\\n' \"$MCPM_LAUNCH_TEST\"\nprintf 'arg=%s\\n' \"$@\"\n").unwrap();
    fs::set_permissions(&java, fs::Permissions::from_mode(0o755)).unwrap();
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    let version = directory.version("base").unwrap();
    let mut options = options();
    options.java.executable = java;
    options.java.version_check = JavaVersionCheck::ExactManifest;
    options.directories.game = Some("isolated game".into());
    options.directories.working = Some("working dir".into());
    options.process.clear_environment = true;
    options
        .process
        .environment
        .insert("MCPM_LAUNCH_TEST".into(), Some("one value".into()));
    options.process.stdin = ProcessStream::Null;
    options.process.stdout = ProcessStream::Piped;
    options.process.stderr = ProcessStream::Piped;
    let plan = directory.plan_launch(&version, &options).unwrap();
    let natives = plan.natives_directory().to_owned();
    let expected = plan.arguments();
    let working = plan.working_directory().to_owned();
    let process = plan.prepare().unwrap().spawn().unwrap();
    assert!(natives.exists());
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(natives.exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&format!("cwd={}\n", working.display())));
    assert!(stdout.contains("env=one value\n"));
    assert_eq!(
        stdout
            .lines()
            .filter_map(|line| line.strip_prefix("arg="))
            .map(OsString::from)
            .collect::<Vec<_>>(),
        expected
    );

    let mut metadata = base();
    metadata["javaVersion"]["majorVersion"] = 21.into();
    profile(root.path(), "base", metadata);
    let version = directory.version("base").unwrap();
    assert!(matches!(
        directory.plan_launch(&version, &options).unwrap().prepare(),
        Err(PreparationError::JavaVersionMismatch {
            actual: 17,
            required: 21,
            ..
        })
    ));
}
