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
        let error = identify::identify(root.path(), script, &BUILTIN_LOADERS).unwrap_err();
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
    let error = identify::identify(&root, "external/run.sh", &BUILTIN_LOADERS).unwrap_err();
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
        let candidates = identify(root.path(), "run.bat", &BUILTIN_LOADERS)
            .unwrap_or_else(|error| panic!("{suffix:?}: {error:#}"));
        assert_eq!(candidates.len(), 1, "{suffix:?}");
        let candidate = &candidates[0];
        assert_eq!(candidate.metadata.minecraft_version, "1.21.1");
        let loader = candidate.metadata.loader.declaration().unwrap();
        assert_eq!(loader.loader_type, "neoforge");
        assert_eq!(loader.version, "21.1.233");
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
        let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
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
    let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("broken.args"), "{message}");
    assert!(message.contains("run.sh"), "{message}");
    assert!(error.chain().count() >= 3, "{message}");
}
