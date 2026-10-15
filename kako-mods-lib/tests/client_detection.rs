use std::fs;

use kako_mods_lib::local::{
    mc::client::{
        MinecraftDirectory, MinecraftVersion, MinecraftVersionError, MinecraftVersionManifest,
    },
    modloader::BUILTIN_LOADERS,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn inspect(value: Value) -> Vec<(String, String)> {
    let manifest: MinecraftVersionManifest = serde_json::from_value(value).unwrap();
    manifest
        .mod_loaders(&BUILTIN_LOADERS)
        .unwrap()
        .into_iter()
        .map(|loader| (loader.loader.id().to_string(), loader.version))
        .collect()
}

#[test]
fn hmcl_library_recognition_and_version_normalization() {
    let cases = [
        (
            "net.fabricmc:fabric-loader:0.19.3",
            Some(("fabric", "0.19.3")),
        ),
        (
            "net.minecraftforge:fmlloader:1.20.1-47.4.16",
            Some(("forge", "47.4.16")),
        ),
        (
            "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
            Some(("forge", "10.13.4.1614")),
        ),
        (
            "net.minecraftforge:forge:26.2-64.0.0",
            Some(("forge", "64.0.0")),
        ),
        (
            "net.minecraftforge:forge:1.20.1-47.0.0-beta",
            Some(("forge", "1.20.1-47.0.0-beta")),
        ),
        (
            "net.neoforged.fancymodloader:core:4.0.42",
            Some(("neoforge", "4.0.42")),
        ),
        (
            "net.neoforged.fancymodloader:loader:1.20.1-47.1.0",
            Some(("neoforge", "47.1.0")),
        ),
        ("net.neoforged:neoforge:21.1.77", None),
        ("net.minecraftforge:eventbus:6.2.33", None),
        ("net.fabricmc:fabric-loader-helper:1.0", None),
    ];
    for (coordinate, expected) in cases {
        let actual = inspect(json!({"id":"test", "libraries":[{"name":coordinate}]}));
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(id, version)| (id.into(), version.into()))
            .collect();
        assert_eq!(actual, expected, "{coordinate}");
    }
}

#[test]
fn neoforge_uses_arguments_and_excludes_forge() {
    for option in ["--fml.neoForgeVersion", "--fml.forgeVersion"] {
        assert_eq!(
            inspect(json!({
                "id":"neo", "mainClass":"cpw.mods.bootstraplauncher.BootstrapLauncher",
                "libraries":[
                    {"name":"net.minecraftforge:fmlloader:1.20.1-47.1.0"},
                    {"name":"net.neoforged.fancymodloader:loader:4.0.42"}
                ],
                "arguments":{"game":[option,"21.1.77"]}
            })),
            vec![("neoforge".into(), "21.1.77".into())]
        );
    }
    // A version argument alone does not establish that NeoForge is installed.
    assert!(
        inspect(json!({"id":"test", "arguments":{"game":["--fml.neoForgeVersion","21.1.77"]}}))
            .is_empty()
    );
}

#[test]
fn neoforge_scans_root_arguments_before_patches_and_ignores_conditional_objects() {
    let mut value = json!({
        "id":"neo", "libraries":[{"name":"net.neoforged.fancymodloader:core:4.0.42"}],
        "arguments":{"game":[{"value":["--fml.neoForgeVersion","wrong"]}, "--fml.neoForgeVersion", "21.1.77"]},
        "patches":[{"id":"support", "arguments":{"game":["--fml.neoForgeVersion","21.1.78"]}}]
    });
    assert_eq!(inspect(value.clone())[0].1, "21.1.77");
    value.as_object_mut().unwrap().remove("arguments");
    assert_eq!(inspect(value.clone())[0].1, "21.1.78");
    value.as_object_mut().unwrap().remove("patches");
    assert_eq!(inspect(value)[0].1, "4.0.42");
}

#[test]
fn legacy_fabric_is_not_reported_as_supported_fabric() {
    assert!(
        inspect(json!({"id":"legacy", "libraries":[
            {"name":"net.fabricmc:fabric-loader:0.16.0"},
            {"name":"net.legacyfabric:intermediary:1.8.9"}
        ]}))
        .is_empty()
    );
}

#[test]
fn visible_patch_metadata_overrides_libraries_and_can_identify_without_libraries() {
    assert_eq!(
        inspect(json!({
            "id":"patched",
            "libraries":[{"name":"net.fabricmc:fabric-loader:0.18.0"}],
            "patches":[
                {"id":"fabric", "version":"0.19.0"},
                {"id":"fabric", "version":"0.19.3"},
                {"id":"fabric", "version":"hidden", "hidden":true},
                {"id":"neoforge", "version":"21.1.77"},
                {"id":"unsupported", "version":"1"}
            ]
        })),
        vec![
            ("fabric".into(), "0.19.3".into()),
            ("neoforge".into(), "21.1.77".into())
        ]
    );
    assert!(
        inspect(
            json!({"id":"hidden", "patches":[{"id":"forge","version":"47.4.16","hidden":true}]})
        )
        .is_empty()
    );
}

#[test]
fn patch_libraries_are_scanned_in_priority_order_and_later_matches_win() {
    assert_eq!(
        inspect(json!({
            "id":"patched",
            "libraries":[{"name":"net.fabricmc:fabric-loader:0.17.0"}],
            "patches":[
                {"id":"high", "priority":30000, "hidden":true, "libraries":[{"name":"net.fabricmc:fabric-loader:0.19.3"}]},
                {"id":"low", "priority":0, "libraries":[{"name":"net.fabricmc:fabric-loader:0.18.0"}]}
            ]
        })),
        vec![("fabric".into(), "0.19.3".into())]
    );
}

#[test]
fn main_class_heuristic_does_not_invent_loader_versions() {
    let root = TempDir::new().unwrap();
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    for (main, modded) in [
        ("net.minecraft.client.main.Main", false),
        ("net.fabricmc.loader.impl.launch.knot.KnotClient", true),
        ("cpw.mods.bootstraplauncher.BootstrapLauncher", true),
        ("net.minecraft.launchwrapper.Launch", true),
        ("org.quiltmc.loader.impl.launch.knot.KnotClient", true),
        ("example.CustomLauncher", false),
    ] {
        write_profile(&root, "base", json!({"id":"base", "mainClass":main}));
        write_profile(&root, "child", json!({"id":"child", "inheritsFrom":"base"}));
        let version = directory.version("child").unwrap();
        assert_eq!(version.is_modded().unwrap(), modded, "{main}");
        assert!(version.mod_loaders(&BUILTIN_LOADERS).unwrap().is_empty());
    }
    write_profile(
        &root,
        "patched",
        json!({
            "id":"patched", "mainClass":"net.minecraft.client.main.Main",
            "patches":[{"id":"fabric", "mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}]
        }),
    );
    assert!(directory.version("patched").unwrap().is_modded().unwrap());
    write_profile(&root, "empty", json!({"id":"empty"}));
    assert!(!directory.version("empty").unwrap().is_modded().unwrap());
    assert!(directory.version("missing").unwrap().is_modded().is_err());
}

fn write_profile(root: &TempDir, name: &str, value: Value) {
    let path = root.path().join("versions").join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join(format!("{name}.json")), value.to_string()).unwrap();
}

#[test]
fn directory_enumeration_retains_broken_profiles_and_resolves_inheritance() {
    let root = TempDir::new().unwrap();
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    assert!(directory.versions().unwrap().is_empty());
    write_profile(
        &root,
        "base",
        json!({"id":"different-json-id", "libraries":[{"name":"net.fabricmc:fabric-loader:0.18.0"}]}),
    );
    write_profile(
        &root,
        "中文 child",
        json!({"id":"child", "inheritsFrom":"base", "libraries":[{"name":"net.fabricmc:fabric-loader:0.19.3"}]}),
    );
    write_profile(&root, "broken", json!({"id":"broken"}));
    fs::write(root.path().join("versions/broken/broken.json"), "{").unwrap();
    fs::create_dir_all(root.path().join("versions/empty")).unwrap();
    fs::write(root.path().join("versions/base/excluded.json"), "{}").unwrap();
    let versions = directory.versions().unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|version| version.name())
            .collect::<Vec<_>>(),
        ["base", "broken", "empty", "中文 child"]
    );
    let MinecraftVersion::Broken(broken) = &versions[1] else {
        panic!("invalid JSON must be retained as Broken");
    };
    assert!(matches!(
        broken.error(),
        MinecraftVersionError::ParseManifest { .. }
    ));
    let MinecraftVersion::Broken(empty) = &versions[2] else {
        panic!("missing manifest must be retained as Broken");
    };
    assert!(
        matches!(empty.error(), MinecraftVersionError::ReadManifest { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    );
    let child = directory.version("中文 child").unwrap();
    assert_eq!(
        child.mod_loaders(&BUILTIN_LOADERS).unwrap()[0].version,
        "0.19.3"
    );
    assert!(child.manifest().unwrap().inherits_from().is_none());
    write_profile(
        &root,
        "base",
        json!({"id":"base", "inheritsFrom":"中文 child"}),
    );
    // Existing objects retain their snapshot; a reload sees the new cycle.
    assert_eq!(
        child.mod_loaders(&BUILTIN_LOADERS).unwrap()[0].version,
        "0.19.3"
    );
    let reloaded = directory.version("中文 child").unwrap();
    let MinecraftVersion::Broken(broken) = reloaded else {
        panic!("expected broken inheritance");
    };
    assert!(matches!(
        broken.error(),
        MinecraftVersionError::InheritanceCycle { .. }
    ));
    assert!(directory.version("../outside").is_err());
}

#[test]
fn broken_parent_errors_are_preserved_and_reload_can_recover() {
    let root = TempDir::new().unwrap();
    let directory = MinecraftDirectory::new(root.path()).unwrap();
    write_profile(
        &root,
        "child",
        json!({"id":"child", "inheritsFrom":"parent"}),
    );
    let child = directory.version("child").unwrap();
    let MinecraftVersion::Broken(broken) = &child else {
        panic!("expected missing parent");
    };
    assert_eq!(broken.name(), "child");
    assert_eq!(broken.path(), root.path().join("versions/child"));
    assert!(
        matches!(broken.error(), MinecraftVersionError::ReadManifest { path, .. }
        if path == &root.path().join("versions/parent/parent.json"))
    );
    assert!(child.is_modded().is_err());
    assert!(child.mod_loaders(&BUILTIN_LOADERS).is_err());

    write_profile(
        &root,
        "parent",
        json!({"id":"parent", "mainClass":"net.minecraft.client.main.Main"}),
    );
    let MinecraftVersion::Normal(normal) = directory.version("child").unwrap() else {
        panic!("expected repaired parent to resolve");
    };
    assert_eq!(normal.manifest().id(), "child");
    assert!(!normal.is_modded());
    assert!(matches!(child, MinecraftVersion::Broken(_)));
}
