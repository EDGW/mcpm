//! Server launch identification, argument expansion, and diagnostic context.

use std::fs;
use tempfile::TempDir;

#[test]
fn current_registration_survives_relocation_without_rewriting() {
    use mcpm_lib::local::mc::server::{MinecraftServer, MinecraftServerMetadata, ServerLoader};

    let parent = TempDir::new().unwrap();
    let original = parent.path().join("original");
    fs::create_dir(&original).unwrap();
    let server = MinecraftServer::create(
        &original,
        MinecraftServerMetadata {
            version: 4,
            minecraft_version: "1.21.1".into(),
            loader: ServerLoader::Vanilla,
        },
    )
    .unwrap();
    let bytes = fs::read(server.metadata_path()).unwrap();
    let moved = parent.path().join("moved");
    fs::rename(&original, &moved).unwrap();
    let loaded = MinecraftServer::load(&moved).unwrap();
    assert_eq!(loaded.path(), moved);
    assert_eq!(loaded.metadata(), server.metadata());
    assert_eq!(fs::read(loaded.metadata_path()).unwrap(), bytes);
}

#[test]
fn identification_rejects_scripts_outside_the_server_without_creating_registration() {
    use mcpm_lib::local::{
        mc::server::{ServerScriptError, identify},
        modloader::BUILTIN_LOADERS,
    };
    for script in [
        "",
        ".",
        "/server/run.sh",
        "../run.sh",
        "sub/../../run.sh",
        r"C:\server\run.bat",
        r"\\host\share\run.bat",
    ] {
        let root = TempDir::new().unwrap();
        let error = identify::identify(root.path(), script, &BUILTIN_LOADERS, None).unwrap_err();
        assert!(
            error.downcast_ref::<ServerScriptError>().is_some(),
            "{script}: {error:#}"
        );
        assert!(!root.path().join(".mcpm").exists());
    }
}

#[cfg(unix)]
#[test]
fn identification_rejects_script_symlinks_escaping_the_server() {
    use mcpm_lib::local::{
        mc::server::{ServerScriptError, identify},
        modloader::BUILTIN_LOADERS,
    };
    use std::os::unix::fs::symlink;
    let parent = TempDir::new().unwrap();
    let root = parent.path().join("server");
    let outside = parent.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.join("external")).unwrap();
    let error = identify::identify(&root, "external/run.sh", &BUILTIN_LOADERS, None).unwrap_err();
    assert!(
        error.downcast_ref::<ServerScriptError>().is_some(),
        "{error:#}"
    );
    assert!(!root.join(".mcpm").exists());
}

#[test]
fn inspection_resolves_loader_without_creating_missing_mod_directory() {
    use mcpm_lib::local::mc::server::MinecraftServer;

    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(root.path().join(".mcpm/server.json"), br#"{"version":4,"minecraft_version":"1.21.1","loader":{"type":"fabric","version":"0.16.10"}}"#).unwrap();
    let inspected = MinecraftServer::inspect(root.path()).unwrap();
    assert_eq!(inspected.resolved_loader().unwrap().version, "0.16.10");
    assert!(inspected.mods().is_none());
    assert!(!root.path().join("mods").exists());
    assert!(MinecraftServer::load(root.path()).unwrap().mods().is_some());
    assert!(root.path().join("mods").is_dir());
}

#[test]
fn batch_invocation_arguments_expand_empty_without_losing_surrounding_arguments() {
    use mcpm_lib::local::{mc::server::identify::identify, modloader::BUILTIN_LOADERS};

    let root = TempDir::new().unwrap();
    let argument_file = "libraries/net/neoforged/neoforge/21.1.233/win_args.txt";
    let arguments = "cpw.mods.bootstraplauncher.BootstrapLauncher --launchTarget neoforgeserver \
        --fml.neoForgeVersion 21.1.233 --fml.mcVersion 1.21.1";
    fs::create_dir_all(root.path().join(argument_file).parent().unwrap()).unwrap();
    fs::write(root.path().join(argument_file), arguments).unwrap();
    fs::write(root.path().join("user_jvm_args.txt"), "-Xmx2G\n").unwrap();
    let executable = r"C:\Program Files\Java\jdk-21\bin\java.exe";

    for (suffix, expected) in [
        ("%*", vec![]),
        ("%* nogui", vec!["nogui"]),
        ("nogui %*", vec!["nogui"]),
        (
            "%* --custom \"value with spaces\"",
            vec!["--custom", "value with spaces"],
        ),
        ("%*\tnogui %* --demo", vec!["nogui", "--demo"]),
        ("\"%*\" nogui", vec!["", "nogui"]),
        ("before%*after nogui", vec!["beforeafter", "nogui"]),
        ("%EXTRA% %* nogui", vec!["--demo", "nogui"]),
    ] {
        fs::write(
            root.path().join("run.bat"),
            format!(
                "@echo off\r\nset \"EXTRA=--demo\"\r\n\"{executable}\" \
                 @user_jvm_args.txt -XX:+UseZGC @{argument_file} {suffix}\r\npause\r\n"
            ),
        )
        .unwrap();
        let candidates = identify(root.path(), "run.bat", &BUILTIN_LOADERS, None)
            .unwrap_or_else(|error| panic!("{suffix:?}: {error:#}"));
        assert_eq!(candidates.len(), 1, "{suffix:?}");
        let candidate = &candidates[0];
        assert_eq!(candidate.launch.executable, executable);
        let expected: Vec<_> = ["-Xmx2G", "-XX:+UseZGC"]
            .into_iter()
            .chain(arguments.split_whitespace())
            .chain(expected)
            .map(str::to_owned)
            .collect();
        assert_eq!(candidate.launch.command.arguments(), expected, "{suffix:?}");
    }
}

#[test]
fn identification_errors_retain_script_archive_member_and_parser_cause() {
    use mcpm_lib::local::{mc::server::identify::identify, modloader::BUILTIN_LOADERS};
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let root = TempDir::new().unwrap();
    let jar = root.path().join("server.jar");
    for (main, member, bytes) in [
        ("net.minecraft.server.Main", "version.json", b"{".as_slice()),
        (
            "net.fabricmc.installer.ServerLauncher",
            "install.properties",
            br"malformed=\u12xz".as_slice(),
        ),
    ] {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            (
                "META-INF/MANIFEST.MF".to_owned(),
                format!("Manifest-Version: 1.0\nMain-Class: {main}\n").into_bytes(),
            ),
            (format!("{}.class", main.replace('.', "/")), vec![]),
            (member.to_owned(), bytes.to_vec()),
        ] {
            archive
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&data).unwrap();
        }
        fs::write(&jar, archive.finish().unwrap().into_inner()).unwrap();
        fs::write(root.path().join("run.sh"), "java -jar server.jar\n").unwrap();
        let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS, None).unwrap_err();
        let message = format!("{error:#}");
        for expected in ["run.sh", ":1", "server.jar", member] {
            assert!(message.contains(expected), "missing {expected}: {message}");
        }
        if member == "version.json" {
            assert!(
                error.downcast_ref::<serde_json::Error>().is_some(),
                "{message}"
            );
        } else {
            assert!(
                message.contains("invalid Java property at line 1"),
                "{message}"
            );
        }
    }
    fs::write(root.path().join("run.sh"), "java @broken.args\n").unwrap();
    fs::write(root.path().join("broken.args"), "\"unfinished\\").unwrap();
    let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS, None).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("broken.args"), "{message}");
    assert!(message.contains("run.sh"), "{message}");
}

/// Minimal modern launch evidence; no Java process or installed libraries are needed.
fn script_candidates(
    script: &str,
    extension: &str,
) -> anyhow::Result<Vec<mcpm_lib::local::mc::server::identify::ServerCandidate>> {
    use mcpm_lib::local::{mc::server::identify::identify, modloader::BUILTIN_LOADERS};
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("server.args"), "cpw.mods.bootstraplauncher.BootstrapLauncher --launchTarget neoforgeserver --fml.neoForgeVersion 21.1.233 --fml.mcVersion 1.21.1").unwrap();
    let name = format!("run.{extension}");
    fs::write(root.path().join(&name), script).unwrap();
    identify(root.path(), &name, &BUILTIN_LOADERS, None)
}

#[test]
fn unsupported_launch_lines_do_not_hide_later_java_commands() {
    for skipped in [
        "java @server.args --precheck || exit 1",
        "java @server.args | tee log.txt",
        "java @server.args && exit 1",
        "java @server.args ; echo ignored",
        "java @server.args &",
        "java @server.args > log.txt",
        "java @server.args < input.txt",
        "(java @server.args)",
        "java @server.args `echo extra`",
        "java @server.args \"$(echo extra)\"",
        "exit 0 | cat",
        "(echo ignored; exit 0)",
        "echo ignored > if",
        "exit 0 > log | cat",
        "echo ignored > cd",
    ] {
        let candidates = script_candidates(&format!("{skipped}\njava @server.args --real\n"), "sh")
            .unwrap_or_else(|error| panic!("{skipped}: {error:#}"));
        assert_eq!(candidates.len(), 1, "{skipped}");
        assert_eq!(candidates[0].launch.line, 2, "{skipped}");
        assert_eq!(
            candidates[0]
                .launch
                .command
                .application
                .last()
                .map(String::as_str),
            Some("--real")
        );
    }
}

#[test]
fn skipped_control_blocks_never_supply_launches_and_resume_after_the_block() {
    for (extension, block) in [
        (
            "sh",
            "if test -f missing; then\nif true; then\njava @server.args --nested\nfi\nelse\njava @server.args --else\nfi",
        ),
        (
            "sh",
            "for item in one two; do\nwhile false; do\njava @server.args --nested\ndone\ndone",
        ),
        ("sh", "until true; do\njava @server.args --loop\ndone"),
        (
            "sh",
            "case value in\nvalue)\njava @server.args --case\n;;\nesac",
        ),
        ("sh", "case value in\nif) : ;;\nesac"),
        ("sh", "case value in\ncd) : ;;\nesac"),
        (
            "bat",
            "if %ERRORLEVEL% NEQ 0 (\n  if exist missing (\n    java @server.args --nested\n  )\n  goto :exit\n) else (\n  java @server.args --else\n)",
        ),
        (
            "bat",
            "for %%x in (one two) do (\n  java @server.args --loop\n)",
        ),
    ] {
        let script = format!("java @server.args --before\n{block}\njava @server.args --after\n");
        let candidates = script_candidates(&script, extension)
            .unwrap_or_else(|error| panic!("{script}: {error:#}"));
        let arguments: Vec<_> = candidates
            .iter()
            .map(|candidate| {
                candidate
                    .launch
                    .command
                    .application
                    .last()
                    .unwrap()
                    .as_str()
            })
            .collect();
        assert_eq!(arguments, ["--before", "--after"], "{script}");
    }
}

#[test]
fn unconditional_exit_stops_but_batch_labels_and_exec_java_do_not() {
    for exit in ["echo stopping; exit 0", "echo stopping; exit 0 2>&1"] {
        let candidates = script_candidates(
            &format!("java @server.args --before\n{exit}\njava @server.args --after\n"),
            "sh",
        )
        .unwrap();
        assert_eq!(candidates.len(), 1, "{exit}");
        assert_eq!(candidates[0].launch.line, 1);
    }
    for (extension, script) in [
        (
            "sh",
            "exec java @server.args --first\njava @server.args --second\n",
        ),
        (
            "bat",
            "java @server.args --first\ngoto :exit\n:exit\njava @server.args --second\n",
        ),
    ] {
        let candidates = script_candidates(script, extension).unwrap();
        assert_eq!(candidates.len(), 2, "{script}");
    }
}

#[test]
fn skipped_state_mutations_do_not_leak_stale_directory_or_variable_values() {
    for (extension, script, expected) in [
        (
            "sh",
            "cd elsewhere || echo failed\njava @server.args\n",
            "script working directory is unknown",
        ),
        (
            "sh",
            "if true; then\ncd elsewhere\nfi\njava @server.args\n",
            "script working directory is unknown",
        ),
        (
            "sh",
            "source helper.sh || echo failed\njava @server.args\n",
            "script working directory is unknown",
        ),
        (
            "sh",
            "ARGS=--old\nARGS=--new || echo failed\njava @server.args $ARGS\n",
            "unresolved shell variable ARGS",
        ),
        (
            "bat",
            "set ARGS=--old\nif exist missing (\nset ARGS=--new\n)\njava @server.args %ARGS%\n",
            "unresolved batch variable ARGS",
        ),
        (
            "bat",
            "set ARGS=--old\nif exist marker set ARGS=--new\njava @server.args %ARGS%",
            "unresolved batch variable ARGS",
        ),
        (
            "sh",
            "if ! cd elsewhere; then\n:\nfi\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "sh",
            "c\"d\" elsewhere || echo failed\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "sh",
            "c\\d elsewhere || echo failed\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "bat",
            "c^d elsewhere || echo failed\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "sh",
            "> log cd elsewhere\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "sh",
            "2> log cd elsewhere\njava @server.args",
            "script working directory is unknown",
        ),
        (
            "bat",
            "echo ignored &> log cd elsewhere\njava @server.args",
            "script working directory is unknown",
        ),
    ] {
        let error = script_candidates(script, extension).unwrap_err();
        assert!(
            format!("{error:#}").contains(expected),
            "{script}: {error:#}"
        );
    }
    let candidates = script_candidates(
        "KEEP=--kept\nOTHER=--old\nOTHER=$(echo new)\njava @server.args $KEEP\n",
        "sh",
    )
    .unwrap();
    assert_eq!(
        candidates[0]
            .launch
            .command
            .application
            .last()
            .map(String::as_str),
        Some("--kept")
    );
}

#[test]
fn unsupported_batch_input_assignment_cannot_reuse_the_old_value() {
    for assignment in [
        "set /p ARGS=Arguments:",
        "set /a ARGS+=1",
        "set /a \"ARGS+=1,OTHER=2\"",
    ] {
        let candidates = script_candidates(&format!("set ARGS=--old\n{assignment}\njava @server.args %ARGS%\njava @server.args --independent"), "bat").unwrap();
        assert_eq!(candidates.len(), 1, "{assignment}");
        assert_eq!(candidates[0].launch.line, 4, "{assignment}");
        assert_eq!(
            candidates[0]
                .launch
                .command
                .application
                .last()
                .map(String::as_str),
            Some("--independent")
        );
    }
}

#[test]
fn skipped_mutations_preserve_unaffected_saved_state() {
    for (extension, script) in [
        (
            "bat",
            "set ARGS=--kept\nsetlocal\nset ARGS=--new || echo failed\nendlocal\njava @server.args %ARGS%",
        ),
        (
            "sh",
            "pushd .\ncd elsewhere || echo failed\npopd\njava @server.args --kept",
        ),
    ] {
        let candidates = script_candidates(script, extension)
            .unwrap_or_else(|error| panic!("{script}: {error:#}"));
        assert_eq!(
            candidates[0]
                .launch
                .command
                .application
                .last()
                .map(String::as_str),
            Some("--kept")
        );
    }
}
