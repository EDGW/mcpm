//! Registration schema boundaries and explicit loader identity across legacy reads and V4 writes.

use std::fs;

use mcpm_lib::local::mc::server::{
    MinecraftServer, MinecraftServerError, MinecraftServerMetadata, SERVER_METADATA_VERSION,
    ServerLoader, ServerModLoader, inspect_metadata,
    upgrade::{self, UpgradeDecision},
};
use serde_json::json;

#[test]
fn legacy_versions_share_v3_shape_without_assuming_vanilla_or_rewriting() {
    for version in 1..=3 {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".mcpm")).unwrap();
        let manifest = root.path().join(".mcpm/server.json");
        let bytes = serde_json::to_vec(&json!({
            "version": version,
            "minecraft_version": "1.12.2",
            "loader": null,
            "old_extension": "retained on disk",
            "start": "obsolete payload is ignored regardless of its shape"
        }))
        .unwrap();
        fs::write(&manifest, &bytes).unwrap();
        let registration = inspect_metadata(root.path()).unwrap();
        assert_eq!(registration.metadata_version, version);
        assert_eq!(registration.metadata.version, SERVER_METADATA_VERSION);
        assert_eq!(registration.metadata.loader, ServerLoader::Unrecognized);
        assert_eq!(registration.metadata.minecraft_version, "1.12.2");
        for result in [
            MinecraftServer::inspect(root.path()),
            MinecraftServer::load(root.path()),
        ] {
            assert!(
                matches!(result.unwrap_err(), MinecraftServerError::UpgradeRequired { version: source, .. } if source == version)
            );
        }
        assert!(matches!(
            MinecraftServer::replace(root.path(), registration.metadata).unwrap_err(),
            MinecraftServerError::UpgradeRequired { .. }
        ));
        assert_eq!(fs::read(&manifest).unwrap(), bytes);
        assert!(!root.path().join("mods").exists());
        let preview = upgrade::prepare(root.path()).unwrap();
        let updated =
            upgrade::apply(root.path(), &preview.revision, UpgradeDecision::Preserve).unwrap();
        assert_eq!(updated.metadata_version(), 4);
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            saved,
            json!({
                "version":4,"minecraft_version":"1.12.2","loader":{"type":"unrecognized"}
            })
        );
    }
}

#[test]
fn v4_states_round_trip_and_unsupported_declarations_keep_their_identity() {
    for loader in [
        ServerLoader::Vanilla,
        ServerLoader::Unrecognized,
        ServerLoader::Modded(ServerModLoader {
            loader_type: "external-loader".into(),
            version: "2.1".into(),
        }),
    ] {
        let root = tempfile::tempdir().unwrap();
        let metadata = MinecraftServerMetadata {
            version: SERVER_METADATA_VERSION,
            minecraft_version: "1.21.1".into(),
            loader,
        };
        MinecraftServer::create(root.path(), metadata.clone()).unwrap();
        let loaded = MinecraftServer::inspect(root.path()).unwrap();
        assert_eq!(loaded.metadata(), &metadata);
        assert_eq!(loaded.metadata_version(), 4);
        assert!(loaded.resolved_loader().is_none());
        assert!(loaded.mods().is_none());
        assert!(!root.path().join("mods").exists());
    }
}

#[test]
fn malformed_or_unsupported_documents_do_not_alias_loader_states() {
    for value in [
        json!({"version":0,"minecraft_version":"1.21.1","loader":null}),
        json!({"version":5,"minecraft_version":"1.21.1","loader":{"type":"vanilla"}}),
        json!({"version":4,"minecraft_version":"1.21.1","loader":null}),
        json!({"version":4,"minecraft_version":"1.21.1"}),
        json!({"version":4,"minecraft_version":"1.21.1","loader":{"type":"vanilla","version":"1"}}),
        json!({"version":4,"minecraft_version":"1.21.1","loader":{"type":"unrecognized","version":"1"}}),
        json!({"version":4,"minecraft_version":"1.21.1","loader":{"type":"forge"}}),
        json!({"version":4,"minecraft_version":"1.21.1","loader":{"type":"forge","version":" "}}),
        json!({"version":3,"minecraft_version":"1.21.1"}),
    ] {
        assert!(
            serde_json::from_value::<MinecraftServerMetadata>(value.clone()).is_err(),
            "{value}"
        );
    }
}
