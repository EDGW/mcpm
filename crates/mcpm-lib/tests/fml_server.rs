//! Server identity boundaries for FML launch arguments and historical Forge archives.

use std::{fs, io::Write, path::Path};

use mcpm_lib::local::{mc::server::identify::identify, modloader::BUILTIN_LOADERS};
use tempfile::tempdir;
use zip::{ZipWriter, write::SimpleFileOptions};

const BOOTSTRAP: &str = "cpw.mods.bootstraplauncher.BootstrapLauncher";
const FORGE_COMPONENT: &str =
    "libraries/net/minecraftforge/fmlloader/1.20.1-47.3.0/fmlloader-1.20.1-47.3.0.jar";
const FORGE_CLASS: &str = "net/minecraftforge/common/ForgeVersion.class";

#[test]
fn fml_family_evidence_does_not_default_or_hide_a_conflict() {
    let root = tempdir().unwrap();
    let versions = "--fml.mcVersion 1.20.1 --fml.forgeVersion 47.3.0";
    for (prefix, suffix, cause) in [
        ("", "", "no recognized loader"),
        (
            "-p libraries/net/minecraftforge/fmlloader/1.20.1-47.3.0/fmlloader-1.20.1-47.3.0.jar/",
            "",
            "no recognized loader",
        ),
        (
            "",
            "--fml.forgeGroup net.minecraftforge --fml.neoForgeVersion 47.3.0",
            "conflicting FML family",
        ),
        (
            "-DlegacyClassPath=libraries/net/neoforged/fancymodloader/loader/47.2.2/loader-47.2.2.jar",
            "--fml.forgeGroup net.minecraftforge",
            "loader-47.2.2.jar",
        ),
    ] {
        fs::write(
            root.path().join("run.sh"),
            format!("java {prefix} {BOOTSTRAP} {versions} {suffix}\n"),
        )
        .unwrap();
        let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
        assert!(format!("{error:#}").contains(cause), "{error:#}");
    }
}

#[test]
fn expanded_maven_args_supply_versions_and_cannot_disagree_with_options() {
    let root = tempdir().unwrap();
    let script = root.path().join("run.sh");
    let relative = "libraries/net/minecraftforge/forge/1.20.1-47.3.0/unix_args.txt";
    let file = root.path().join(relative);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, format!("-p {FORGE_COMPONENT}\n{BOOTSTRAP}\n")).unwrap();
    fs::write(&script, format!("java @{relative}\n")).unwrap();
    assert_identity(root.path(), "forge", "1.20.1", "47.3.0");
    fs::write(
        &script,
        format!("java @{relative} --fml.forgeVersion=47.4.0\n"),
    )
    .unwrap();
    let error = format!(
        "{:#}",
        identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err()
    );
    for expected in ["47.3.0", "47.4.0", relative, "--fml.forgeVersion"] {
        assert!(error.contains(expected), "{error}");
    }

    let relative = "libraries/net/minecraftforge/forge/unsplit/unix_args.txt";
    let file = root.path().join(relative);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, format!("-p {FORGE_COMPONENT}\n{BOOTSTRAP}\n")).unwrap();
    fs::write(&script, format!("java @{relative}\n")).unwrap();
    let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
    assert!(
        format!("{error:#}").contains("invalid FML distribution version"),
        "{error:#}"
    );

    let relative = "libraries/net/minecraftforge/forge/1.21-pre1-asdfjk/unix_args.txt";
    let file = root.path().join(relative);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, format!("-p {FORGE_COMPONENT}\n{BOOTSTRAP}\n")).unwrap();
    fs::write(
        &script,
        format!("java @{relative} --fml.mcVersion=1.21-pre1\n"),
    )
    .unwrap();
    assert_identity(root.path(), "forge", "1.21-pre1", "asdfjk");

    fs::create_dir_all(root.path().join("net/neoforged/neoforge")).unwrap();
    let mut misleading_paths = vec!["net/neoforged/neoforge/../unix_args.txt"];
    if cfg!(unix) {
        misleading_paths.push(r"net\neoforged\neoforge\21.1.265\unix_args.txt");
    }
    for relative in misleading_paths {
        fs::write(
            root.path().join(relative),
            format!("{BOOTSTRAP}\n--fml.forgeGroup net.neoforged\n--fml.mcVersion 1.21.1\n"),
        )
        .unwrap();
        fs::write(&script, format!("java '@{relative}'\n")).unwrap();
        let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
        assert!(
            format!("{error:#}").contains("loader version is missing"),
            "{error:#}"
        );
    }
}

#[test]
fn server_main_uses_manifest_continuations_without_importing_defaults() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("run.sh"), "java -jar entry.jar nogui\n").unwrap();
    let manifest = concat!(
        "Manifest-Version: 1.0\r\nMain-Class: net.minecraftforge.server.ServerMain\r\n",
        "ServerLaunchArgs: --launchTarget fmlserver --fml.forgeVers\r\n",
        " ion 36.2.39\r\n",
        "  --fml.forgeGroup net.minecraftforge --fml.mcVersion 1.16.5\r\n\r\n",
    );
    write_jar(root.path(), [("META-INF/MANIFEST.MF", manifest.as_bytes())]);
    let candidates = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    let metadata = &candidates[0].metadata;
    assert_eq!(metadata.minecraft_version, "1.16.5");
    assert_eq!(metadata.loader.declaration().unwrap().version, "36.2.39");
    assert_eq!(candidates[0].launch.command.application, ["nogui"]);

    let conflicting = concat!(
        "Manifest-Version: 1.0\nMain-Class: net.minecraftforge.server.ServerMain\n",
        "ServerLaunchArgs: --fml.forgeGroup net.neoforged\n\n",
    );
    write_jar(
        root.path(),
        [("META-INF/MANIFEST.MF", conflicting.as_bytes())],
    );
    let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
    assert!(
        format!("{error:#}")
            .contains("Forge launcher declares the NeoForge family from --fml.forgeGroup"),
        "{error:#}"
    );
}

#[test]
fn shim_versions_are_checked_even_when_explicit_versions_are_complete() {
    let root = tempdir().unwrap();
    let manifest = b"Manifest-Version: 1.0\nMain-Class: net.minecraftforge.bootstrap.shim.Main\n\n";
    let defaults = b"Arguments=--launchTarget forge_client\n";
    let list = b"hash\tcoordinate\tnet/minecraftforge/fmlloader/1.20.4-49.2.0/fmlloader-1.20.4-49.2.0.jar\n\
        hash\tcoordinate\tnet/minecraftforge/forge/1.20.4-49.2.0/forge-1.20.4-49.2.0-server.jar\n";
    let write_shim = |list: &[u8]| {
        write_jar(
            root.path(),
            [
                ("META-INF/MANIFEST.MF", manifest.as_slice()),
                ("bootstrap-shim.properties", defaults.as_slice()),
                ("bootstrap-shim.list", list),
            ],
        );
    };
    write_shim(list);
    let script = root.path().join("run.sh");
    let launch =
        "java -jar entry.jar --launchTarget forge_server --fml.mcVersion 1.20.4 --fml.forgeVersion";
    fs::write(&script, format!("{launch} 49.2.0\n")).unwrap();
    assert_identity(root.path(), "forge", "1.20.4", "49.2.0");
    fs::write(&script, format!("{launch} 49.3.0\n")).unwrap();
    let error = format!(
        "{:#}",
        identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err()
    );
    for expected in [
        "49.3.0",
        "forge-1.20.4-49.2.0-server.jar",
        "bootstrap-shim.list line 2",
        "--fml.forgeVersion",
    ] {
        assert!(error.contains(expected), "{error}");
    }

    let first_row_end = list.iter().position(|&byte| byte == b'\n').unwrap();
    let malformed = [&list[..=first_row_end], b"hash\tcoordinate\t\xff\n"].concat();
    write_shim(&malformed);
    let error = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err();
    assert!(
        format!("{error:#}").contains("!bootstrap-shim.list at line 2"),
        "{error:#}"
    );
}

#[test]
fn legacy_forge_constants_take_precedence_over_the_vanilla_entry_class() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("run.sh"), "java -jar entry.jar\n").unwrap();
    let manifest = b"Manifest-Version: 1.0\nMain-Class: net.minecraft.server.MinecraftServer\n\n";
    for (field_count, static_build, mc, property, expected) in [
        (4, true, None, "1.5.2", None),
        (
            3,
            true,
            None,
            "1.5.2",
            Some("missing integer constant buildVersion"),
        ),
        (
            4,
            false,
            None,
            "1.5.2",
            Some("invalid integer constant buildVersion"),
        ),
        (4, true, Some("1.5.2"), "1.5.1", Some("fmlbuild.mcversion")),
    ] {
        let class = forge_version_class(field_count, static_build, mc);
        let properties = format!("fmlbuild.mcversion={property}\n");
        write_jar(
            root.path(),
            [
                ("META-INF/MANIFEST.MF", manifest.as_slice()),
                (FORGE_CLASS, &class),
                ("fmlversion.properties", properties.as_bytes()),
            ],
        );
        if let Some(expected) = expected {
            let error = format!(
                "{:#}",
                identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap_err()
            );
            assert!(
                error.contains(FORGE_CLASS) && error.contains(expected),
                "{error}"
            );
        } else {
            assert_identity(root.path(), "forge", "1.5.2", "7.8.1.738");
        }
    }
}

fn assert_identity(root: &Path, family: &str, mc: &str, version: &str) {
    let candidates = identify(root, "run.sh", &BUILTIN_LOADERS).unwrap();
    let [candidate] = candidates.as_slice() else {
        panic!("expected one launch: {candidates:?}")
    };
    let loader = candidate.metadata.loader.declaration().unwrap();
    assert_eq!(
        (
            &*loader.loader_type,
            &*candidate.metadata.minecraft_version,
            &*loader.version
        ),
        (family, mc, version)
    );
}

fn write_jar<'a>(root: &Path, members: impl IntoIterator<Item = (&'a str, &'a [u8])>) {
    let mut zip = ZipWriter::new(fs::File::create(root.join("entry.jar")).unwrap());
    for (name, bytes) in members {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// Minimal Java 5 class: constant fields, unrelated fields, no methods.
fn forge_version_class(field_count: usize, static_build: bool, mc: Option<&str>) -> Vec<u8> {
    let mut pool = vec![vec![]]; // Classfile indices start at one.
    let mut add = |entry: Vec<u8>| {
        let index = pool.len() as u16;
        pool.push(entry);
        index
    };
    fn utf8(text: &str) -> Vec<u8> {
        [
            vec![1],
            (text.len() as u16).to_be_bytes().to_vec(),
            text.as_bytes().to_vec(),
        ]
        .concat()
    }
    let name = add(utf8(FORGE_CLASS.strip_suffix(".class").unwrap()));
    let this = add([&[7], name.to_be_bytes().as_slice()].concat());
    let name = add(utf8("java/lang/Object"));
    let superclass = add([&[7], name.to_be_bytes().as_slice()].concat());
    let constant = add(utf8("ConstantValue"));
    let integer = add(utf8("I"));
    let mut fields = Vec::new();
    for (name, value) in [
        ("majorVersion", 7i32),
        ("minorVersion", 8),
        ("revisionVersion", 1),
        ("buildVersion", 738),
    ]
    .into_iter()
    .take(field_count)
    {
        let flags = if name == "buildVersion" && !static_build {
            0x11
        } else {
            0x19
        };
        let name = add(utf8(name));
        let value = add([&[3], value.to_be_bytes().as_slice()].concat());
        fields.push((flags, name, integer, value));
    }
    if let Some(mc) = mc {
        let name = add(utf8("mcVersion"));
        let descriptor = add(utf8("Ljava/lang/String;"));
        let text = add(utf8(mc));
        let value = add([&[8], text.to_be_bytes().as_slice()].concat());
        fields.push((0x19, name, descriptor, value));
    }
    let extra = add(utf8("extra"));
    let long = add(utf8("J"));
    let mut bytes = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 49];
    bytes.extend((pool.len() as u16).to_be_bytes());
    bytes.extend(pool.into_iter().flatten());
    for value in [0x21, this, superclass, 0, fields.len() as u16 + 2] {
        bytes.extend(value.to_be_bytes());
    }
    for (flags, name, descriptor, value) in fields {
        for item in [flags, name, descriptor, 1, constant, 0, 2, value] {
            bytes.extend(item.to_be_bytes());
        }
    }
    for descriptor in [integer, long] {
        for item in [1u16, extra, descriptor, 0] {
            bytes.extend(item.to_be_bytes());
        }
    }
    bytes.extend([0, 0, 0, 0]); // No methods or class attributes.
    bytes
}
