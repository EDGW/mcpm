//! Import preservation, configuration ownership, runtime overrides and publication boundaries.

use super::*;
use crate::local::{
    mc::server::{MinecraftServer, identify},
    modloader::BUILTIN_LOADERS,
};

fn candidate(root: &Path, options: &str) -> identify::ServerCandidate {
    fs::write(root.join("run.sh"), format!("java {options} cpw.mods.bootstraplauncher.BootstrapLauncher --fml.mcVersion 1.21.1 --fml.forgeVersion 52.0.1 --launchTarget forge_server nogui\n")).unwrap();
    identify::identify(root, "run.sh", &BUILTIN_LOADERS)
        .unwrap()
        .remove(0)
}

#[test]
fn imported_values_are_active_and_absent_values_remain_comments() {
    let root = tempfile::tempdir().unwrap();
    let candidate = candidate(
        root.path(),
        "-Xmx24G -Xms1G -Xss20M -XX:+UseG1GC -Dloader.fixed=yes -cp '-Xmx2G'",
    );
    let text = candidate.configuration.template().unwrap();
    let conf = decode_conf(&text).unwrap();
    assert_eq!(conf.java.executable.as_deref(), Some("java"));
    assert_eq!(conf.jvm.heap.maximum.as_deref(), Some("24G"));
    assert_eq!(conf.jvm.heap.initial.as_deref(), Some("1G"));
    assert_eq!(conf.jvm.thread_stack.as_deref(), Some("20M"));
    assert_eq!(conf.jvm.gc.collector, Some(Collector::G1));
    assert!(conf.jvm.heap.maximum_percentage.is_none());
    assert!(conf.jvm.memory.pre_touch.is_none());
    assert_eq!(
        candidate.configuration.launch.jvm.extra_arguments,
        ["-Dloader.fixed=yes", "-cp", "-Xmx2G"]
    );
    let empty = decode_conf(&import::default_template().unwrap()).unwrap();
    assert!(empty.jvm_arguments().unwrap().is_empty());
    assert!(empty.server_arguments().is_empty());
    assert!(empty.java.executable.is_none());
}

#[test]
fn ordered_unlock_dependencies_and_unpaired_young_sizes_stay_in_one_memory_region() {
    let root = tempfile::tempdir().unwrap();
    let candidate = candidate(
        root.path(),
        "-XX:+UnlockExperimentalVMOptions -XX:+UseEpsilonGC -XX:-UnlockExperimentalVMOptions -XX:NewSize=128M -XX:MaxNewSize=256M",
    );
    let conf = decode_conf(&candidate.configuration.template().unwrap()).unwrap();
    assert_eq!(
        conf.jvm.memory.extra_arguments,
        [
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+UseEpsilonGC",
            "-XX:-UnlockExperimentalVMOptions",
            "-XX:NewSize=128M",
            "-XX:MaxNewSize=256M"
        ]
    );
    assert!(conf.jvm.heap.young.is_none());
    assert!(conf.jvm.gc.collector.is_none());
    assert!(
        candidate
            .configuration
            .launch
            .jvm
            .extra_arguments
            .is_empty()
    );
    for text in [
        "[java]\n[jvm.heap]\nmaximum='1G'\n[jvm.memory]\nextraArguments=['-XX:MaxHeapSize=2G']",
        "[java]\n[jvm.gc]\nstringDeduplication=true\n[jvm.memory]\nextraArguments=['-XX:-UseStringDeduplication']",
        "[java]\n[jvm.heap]\ninitial='2G'\nmaximum='1G'",
    ] {
        assert!(decode_conf(text).is_ok(), "{text}");
    }
    assert!(decode_conf("[java]\n[jvm.gc]\ncollector='parallel'\n[jvm.memory]\nextraArguments=['-XX:+UseParallelOldGC']").is_ok());
}

#[test]
fn import_retains_existing_conf_bytes_and_invalid_preserved_conf_blocks_publication() {
    let root = tempfile::tempdir().unwrap();
    let initial = candidate(root.path(), "-Xmx2G");
    publish(
        root.path(),
        &initial.metadata,
        Some(&initial.configuration),
        OverwritePolicy::default(),
    )
    .unwrap();
    let conf = b"# user formatting\r\n[java]\r\n# executable = 'other'\r\n\r\n[jvm.heap]\r\nmaximum='3G'\r\n";
    fs::write(root.path().join("conf.toml"), conf).unwrap();
    let next = candidate(root.path(), "-Xmx4G");
    let force = OverwritePolicy {
        replace_registration: true,
        replace_conf: false,
    };
    publish(
        root.path(),
        &next.metadata,
        Some(&next.configuration),
        force,
    )
    .unwrap();
    assert_eq!(fs::read(root.path().join("conf.toml")).unwrap(), conf);
    let old_launch = fs::read(root.path().join("launch.toml")).unwrap();
    fs::write(root.path().join("conf.toml"), "[java]\nunknown = true").unwrap();
    assert!(
        publish(
            root.path(),
            &next.metadata,
            Some(&next.configuration),
            force
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.path().join("launch.toml")).unwrap(),
        old_launch
    );
    assert!(
        publish(
            root.path(),
            &next.metadata,
            Some(&next.configuration),
            OverwritePolicy {
                replace_registration: false,
                replace_conf: true
            }
        )
        .is_err()
    );
    publish(
        root.path(),
        &next.metadata,
        Some(&next.configuration),
        OverwritePolicy {
            replace_registration: true,
            replace_conf: true,
        },
    )
    .unwrap();
    assert_eq!(
        decode_conf(&fs::read_to_string(root.path().join("conf.toml")).unwrap())
            .unwrap()
            .jvm
            .heap
            .maximum
            .as_deref(),
        Some("4G")
    );
    assert!(
        MinecraftServer::inspect(root.path())
            .unwrap()
            .metadata()
            .start
            .is_none()
    );
}

#[test]
fn runtime_overrides_follow_schema_and_order_list_operations_before_final_validation() {
    let source = "[java]\n[jvm.heap]\ninitial='3G'\nmaximum='2G'\n[jvm]\nextraArguments=['-Doriginal=yes']\n";
    let mut overrides = ConfigurationOverrides::default();
    for assignment in [
        "jvm.heap.initial=1G",
        "jvm.heap.maximum=4G",
        "jvm.gc.collector=g1",
        "server.world=1e3",
        "server.port=25566",
        "server.nogui=false",
        "jvm.heap.maximumRAM=8589934592",
        "jvm.extraArguments=-Ddiscard=yes",
        "jvm.extraArguments:=",
        "jvm.extraArguments=-Done=1",
        "jvm.extraArguments+-Dtwo=2",
        "jvm.extraArguments+=['-Dthree=3', '-Dfour=4']",
        "application.extraArguments:=['', '中文 world']",
    ] {
        overrides.parse(assignment).unwrap();
    }
    let effective = decode_conf_with_overrides(source, &overrides).unwrap();
    assert_eq!(effective.jvm.heap.maximum.as_deref(), Some("4G"));
    assert_eq!(
        effective.jvm.heap.maximum_ram.as_deref(),
        Some("8589934592")
    );
    assert_eq!(effective.server.world.as_deref(), Some("1e3"));
    assert_eq!(effective.server.port, Some(25566));
    assert_eq!(effective.server.nogui, Some(false));
    assert_eq!(effective.jvm.gc.collector, Some(Collector::G1));
    assert_eq!(
        effective.jvm.extra_arguments,
        ["-Done=1", "-Dtwo=2", "-Dthree=3", "-Dfour=4"]
    );
    assert_eq!(effective.application.extra_arguments, ["", "中文 world"]);
    assert!(overrides.set("jvm.heap.maximum", "8G").is_err());
    for invalid in [
        "unknown.field=1",
        "jvm={}",
        "server.port+=1",
        "server.port=hello",
        "jvm.extraArguments:=[1]",
    ] {
        let mut changes = ConfigurationOverrides::default();
        if changes.parse(invalid).is_ok() {
            assert!(
                decode_conf_with_overrides("[java]", &changes).is_err(),
                "{invalid}"
            );
        }
    }
    for invalid in [
        "jvm.heap.maximum=auto",
        "jvm.gc.collector=auto",
        "jvm.memory.auto.heapFraction=0.7",
        "jvm.memory.auto.reserve=1G",
        "jvm.extraArguments=-Xmx2G",
    ] {
        let mut changes = ConfigurationOverrides::default();
        changes.parse(invalid).unwrap();
        assert!(
            decode_conf_with_overrides("[java]\n[jvm.heap]\nmaximum='1G'", &changes).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn imports_preserve_foreign_absolute_java_and_application_operand_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let mut launch = candidate(root.path(), "").launch;
    launch.path_separator = ';';
    launch.directory.push("run");
    for executable in [r"C:\jdk\bin\java.exe", r"\\host\share\java.exe"] {
        launch.executable = executable.into();
        let imported = draft(root.path(), &launch).unwrap();
        let conf = decode_conf(&imported.template().unwrap()).unwrap();
        assert_eq!(conf.java.executable.as_deref(), Some(executable));
    }
    launch.command.application = [
        "--universe",
        "nogui",
        "--config",
        "--port",
        "--world",
        "world",
        "--port",
        "25566",
        "--demo",
    ]
    .map(String::from)
    .into();
    let imported = draft(root.path(), &launch).unwrap();
    let conf = decode_conf(&imported.template().unwrap()).unwrap();
    assert_eq!(conf.server.nogui, None);
    assert_eq!(conf.server.port, Some(25566));
    assert_eq!(conf.server.world.as_deref(), Some("world"));
    assert_eq!(
        imported.launch.application.extra_arguments,
        ["--universe", "nogui", "--config", "--port", "--demo"]
    );
    launch.command.application = ["--custom", "nogui", "--port", "25566"]
        .map(String::from)
        .into();
    let imported = draft(root.path(), &launch).unwrap();
    assert_eq!(
        imported.launch.application.extra_arguments,
        launch.command.application
    );
    assert!(
        decode_conf(&imported.template().unwrap())
            .unwrap()
            .server
            .port
            .is_none()
    );
    launch.command.jvm = [
        "-Dnote=IgnoreUnrecognizedVMOptions",
        "-XX:+PrintCompilation",
    ]
    .map(String::from)
    .into();
    assert!(draft(root.path(), &launch).is_ok());
    for arguments in [
        ["-XX:+UnlockDiagnosticVMOptions", "-XX:+DebugNonSafepoints"],
        [
            "-XX:+IgnoreUnrecognizedVMOptions",
            "-XX:+FutureUnknownOption",
        ],
    ] {
        launch.command.jvm = arguments.map(String::from).into();
        assert!(
            draft(root.path(), &launch)
                .unwrap_err()
                .to_string()
                .contains("unlock/ignore ordering")
        );
    }
}

#[test]
fn imports_preserve_raw_repetition_and_unrepresentable_application_values() {
    let root = tempfile::tempdir().unwrap();
    for jvm in [
        "-Xms0 -Xmxnot-a-Java-size -Xss0 -XX:MaxDirectMemorySize=0",
        "-Xmx1G -Xmx2G -XX:+UseG1GC -XX:+UseZGC",
    ] {
        let candidate = candidate(root.path(), jvm);
        let conf = decode_conf(&candidate.configuration.template().unwrap()).unwrap();
        assert_eq!(conf.jvm_arguments().unwrap(), candidate.launch.command.jvm);
    }
    let mut launch = candidate(root.path(), "").launch;
    for application in [
        vec!["--port", "not-a-port", "--world", ""],
        vec!["--port", "25565", "--port", "25566"],
        vec!["nogui", "--config"],
    ] {
        launch.command.application = application.into_iter().map(String::from).collect();
        let draft = draft(root.path(), &launch).unwrap();
        assert_eq!(
            draft.launch.application.extra_arguments,
            launch.command.application
        );
        let conf = decode_conf(&draft.template().unwrap()).unwrap();
        assert!(conf.server_arguments().is_empty());
    }
}

#[test]
fn independent_exports_keep_per_command_environment_snapshots() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("child.sh"), "java cpw.mods.bootstraplauncher.BootstrapLauncher --fml.mcVersion 1.21.1 --fml.forgeVersion 52.0.1\n").unwrap();
    fs::write(root.path().join("run.sh"), "EXPORTED='first value'\nLOCAL_ONLY=private\nexport EXPORTED LATER\nLATER=later\nsh ./child.sh\nEXPORTED='second value'\njava cpw.mods.bootstraplauncher.BootstrapLauncher --fml.mcVersion 1.21.1 --fml.forgeVersion 52.0.1\n").unwrap();
    let candidates = identify::identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    assert_eq!(candidates.len(), 2);
    for (candidate, value) in candidates.iter().zip(["first value", "second value"]) {
        let environment = &candidate.configuration.launch.process.environment;
        assert_eq!(environment.get("EXPORTED").map(String::as_str), Some(value));
        assert_eq!(environment.get("LATER").map(String::as_str), Some("later"));
        assert!(!environment.contains_key("LOCAL_ONLY"));
    }
}
