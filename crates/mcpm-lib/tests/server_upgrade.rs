//! Static upgrade evidence, user decisions, optimistic revisions and metadata-only publication.

use std::{fs, io::Write, path::Path};

use mcpm_lib::local::mc::server::{
    MinecraftServer, ServerLoader,
    upgrade::{self, UpgradeApplyError, UpgradeDecision},
};
use serde_json::json;

fn legacy(root: &Path, version: u32, loader: serde_json::Value) -> Vec<u8> {
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    let bytes = serde_json::to_vec(&json!({
        "version":version,"minecraft_version":"1.12.2","loader":loader
    }))
    .unwrap();
    fs::write(root.join(".mcpm/server.json"), &bytes).unwrap();
    bytes
}

fn launch(root: &Path, entry: &str) {
    fs::write(root.join("launch.toml"), format!("[entry]\n{entry}\n")).unwrap();
}

fn jar(path: &Path, main_class: &str) {
    let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
    archive
        .start_file(
            "META-INF/MANIFEST.MF",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    write!(
        archive,
        "Manifest-Version: 1.0\nMain-Class: {main_class}\n\n"
    )
    .unwrap();
    archive.finish().unwrap();
}

#[test]
fn legacy_confirmation_can_promote_or_preserve_without_touching_configuration() {
    for version in 1..=3 {
        for (decision, expected) in [
            (UpgradeDecision::Preserve, ServerLoader::Unrecognized),
            (UpgradeDecision::UseVanilla, ServerLoader::Vanilla),
        ] {
            let root = tempfile::tempdir().unwrap();
            let old = legacy(root.path(), version, json!(null));
            launch(
                root.path(),
                "type='class'\nmainClass='net.minecraft.server.Main'",
            );
            fs::write(root.path().join("conf.toml"), "invalid unrelated tuning").unwrap();
            let before = fs::read(root.path().join("launch.toml")).unwrap();
            let preview = upgrade::prepare(root.path()).unwrap();
            assert_eq!(preview.from_version, version);
            assert_eq!(
                preview.vanilla.unwrap().main_class,
                "net.minecraft.server.Main"
            );
            assert_eq!(
                fs::read(root.path().join(".mcpm/server.json")).unwrap(),
                old
            );
            let updated = upgrade::apply(root.path(), &preview.revision, decision).unwrap();
            assert_eq!(updated.metadata_version(), 4);
            assert_eq!(updated.metadata().loader, expected);
            assert_eq!(updated.metadata().minecraft_version, "1.12.2");
            assert_eq!(fs::read(root.path().join("launch.toml")).unwrap(), before);
            assert_eq!(
                fs::read_to_string(root.path().join("conf.toml")).unwrap(),
                "invalid unrelated tuning"
            );
            assert!(!root.path().join("mods").exists());
            assert!(!root.path().join(".git").exists());
            assert!(!root.path().join(".mcpm/.gitignore").exists());
            assert!(!root.path().join(".mcpm/mcpm.json").exists());
        }
    }
}

#[test]
fn jar_evidence_uses_working_directory_without_requiring_version_json() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path(), 3, json!(null));
    fs::create_dir(root.path().join("work")).unwrap();
    jar(
        &root.path().join("work/server.jar"),
        "net.minecraft.bundler.Main",
    );
    jar(&root.path().join("server.jar"), "example.NotVanilla");
    fs::write(
        root.path().join("launch.toml"),
        "[process]\nworkingDirectory='work'\n[entry]\ntype='jar'\npath='server.jar'\n",
    )
    .unwrap();
    let preview = upgrade::prepare(root.path()).unwrap();
    let evidence = preview.vanilla.unwrap();
    assert_eq!(evidence.main_class, "net.minecraft.bundler.Main");
    assert!(evidence.jar.unwrap().ends_with("work/server.jar"));
    upgrade::apply(root.path(), &preview.revision, UpgradeDecision::UseVanilla).unwrap();
    assert_eq!(
        MinecraftServer::inspect(root.path())
            .unwrap()
            .metadata()
            .loader,
        ServerLoader::Vanilla
    );
}

#[test]
fn nonvanilla_and_implicit_module_entries_never_authorize_promotion() {
    for entry in [
        "type='class'\nmainClass='net.minecraftforge.fml.relauncher.ServerLaunchWrapper'",
        "type='module'\nmodule='example.server'",
    ] {
        let root = tempfile::tempdir().unwrap();
        let old = legacy(root.path(), 3, json!(null));
        launch(root.path(), entry);
        let preview = upgrade::prepare(root.path()).unwrap();
        assert!(preview.vanilla.is_none());
        let error = upgrade::apply(root.path(), &preview.revision, UpgradeDecision::UseVanilla)
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<UpgradeApplyError>(),
            Some(UpgradeApplyError::MissingVanillaEvidence)
        ));
        assert_eq!(
            fs::read(root.path().join(".mcpm/server.json")).unwrap(),
            old
        );
        let server =
            upgrade::apply(root.path(), &preview.revision, UpgradeDecision::Preserve).unwrap();
        assert_eq!(server.metadata().loader, ServerLoader::Unrecognized);
    }
    let root = tempfile::tempdir().unwrap();
    legacy(root.path(), 3, json!(null));
    launch(
        root.path(),
        "type='module'\nmodule='minecraft'\nmainClass='net.minecraft.server.Main'",
    );
    assert!(upgrade::prepare(root.path()).unwrap().vanilla.is_some());
}

#[test]
fn missing_configuration_preserves_but_corrupt_evidence_fails_without_writing() {
    let root = tempfile::tempdir().unwrap();
    legacy(root.path(), 3, json!(null));
    let preview = upgrade::prepare(root.path()).unwrap();
    assert!(preview.vanilla.is_none());
    upgrade::apply(root.path(), &preview.revision, UpgradeDecision::Preserve).unwrap();
    for text in [
        "not valid TOML [",
        "[entry]\ntype='jar'\npath='missing.jar'\n",
    ] {
        let old = legacy(root.path(), 3, json!(null));
        fs::write(root.path().join("launch.toml"), text).unwrap();
        assert!(upgrade::prepare(root.path()).is_err());
        assert_eq!(
            fs::read(root.path().join(".mcpm/server.json")).unwrap(),
            old
        );
    }
}

#[test]
fn stale_registration_configuration_or_manifest_rejects_previous_decision() {
    for change in ["registration", "configuration", "manifest"] {
        let root = tempfile::tempdir().unwrap();
        legacy(root.path(), 3, json!(null));
        launch(root.path(), "type='jar'\npath='server.jar'");
        jar(&root.path().join("server.jar"), "net.minecraft.server.Main");
        let preview = upgrade::prepare(root.path()).unwrap();
        match change {
            "registration" => {
                legacy(root.path(), 2, json!(null));
            }
            "configuration" => {
                fs::write(
                    root.path().join("launch.toml"),
                    "[entry]\ntype='jar'\npath='./server.jar'\n",
                )
                .unwrap();
            }
            "manifest" => {
                jar(&root.path().join("server.jar"), "example.NotVanilla");
            }
            _ => unreachable!(),
        }
        let before = fs::read(root.path().join(".mcpm/server.json")).unwrap();
        let error = upgrade::apply(root.path(), &preview.revision, UpgradeDecision::UseVanilla)
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<UpgradeApplyError>(),
            Some(UpgradeApplyError::StalePreview)
        ));
        assert_eq!(
            fs::read(root.path().join(".mcpm/server.json")).unwrap(),
            before
        );
    }
}

#[test]
fn concrete_loader_is_preserved_and_current_registration_is_a_bytewise_noop() {
    let root = tempfile::tempdir().unwrap();
    legacy(
        root.path(),
        3,
        json!({"type":"unsupported-loader","version":"2.0"}),
    );
    fs::write(root.path().join("launch.toml"), "broken irrelevant TOML [").unwrap();
    let preview = upgrade::prepare(root.path()).unwrap();
    let server = upgrade::apply(root.path(), &preview.revision, UpgradeDecision::Preserve).unwrap();
    let declaration = server.metadata().loader.declaration().unwrap();
    assert_eq!(declaration.loader_type, "unsupported-loader");
    assert_eq!(declaration.version, "2.0");
    let before = fs::read(root.path().join(".mcpm/server.json")).unwrap();
    let current = upgrade::prepare(root.path()).unwrap();
    assert_eq!(current.from_version, 4);
    upgrade::apply(root.path(), &current.revision, UpgradeDecision::Preserve).unwrap();
    assert_eq!(
        fs::read(root.path().join(".mcpm/server.json")).unwrap(),
        before
    );
}
