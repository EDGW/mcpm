use std::{fs, io};

use kako_mods_lib::local::mc::server::{
    MinecraftServer, MinecraftServerError, MinecraftServerMetadata, ServerModLoader,
};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn metadata_round_trip_and_existing_metadata_is_preserved() {
    let root = TempDir::new().unwrap();
    for loader in [
        None,
        Some(ServerModLoader {
            loader_type: "fabric".into(),
            version: "0.19.3".into(),
        }),
    ] {
        let path = root.path().join(if loader.is_some() {
            "modded"
        } else {
            "vanilla"
        });
        fs::create_dir(&path).unwrap();
        let metadata = MinecraftServerMetadata {
            version: 1,
            minecraft_version: "1.21.1".into(),
            loader,
            start: None,
        };
        let server = MinecraftServer::create(&path, metadata.clone()).unwrap();
        let bytes = fs::read(server.metadata_path()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("type").is_none());
        assert_eq!(value["version"], 1);
        assert_eq!(value["minecraft_version"], "1.21.1");
        assert_eq!(value["loader"].is_null(), metadata.loader.is_none());
        assert_eq!(MinecraftServer::load(&path).unwrap().metadata(), &metadata);
        assert_eq!(server.path(), path);
        assert_eq!(server.name(), path.file_name().unwrap().to_str().unwrap());
        let identified = server.loader();
        assert_eq!(identified.is_some(), metadata.loader.is_some());
        if let Some(info) = identified {
            assert_eq!(info.loader.id().as_str(), "fabric");
            assert_eq!(info.version, "0.19.3");
        }

        let replacement = MinecraftServerMetadata {
            minecraft_version: "changed".into(),
            ..metadata
        };
        assert!(matches!(
            MinecraftServer::create(&path, replacement),
            Err(MinecraftServerError::WriteMetadata { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(fs::read(server.metadata_path()).unwrap(), bytes);
    }
}

#[test]
fn unknown_loader_is_not_identified_and_dot_path_keeps_directory_name() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("example-server");
    fs::create_dir(&path).unwrap();
    let metadata = MinecraftServerMetadata {
        version: 1,
        minecraft_version: "1.20.1".into(),
        start: None,
        loader: Some(ServerModLoader {
            loader_type: "custom".into(),
            version: "1.0".into(),
        }),
    };
    MinecraftServer::create(&path, metadata.clone()).unwrap();
    let server = MinecraftServer::load(path.join(".")).unwrap();
    assert_eq!(server.name(), "example-server");
    assert!(server.loader().is_none());
    assert_eq!(server.metadata(), &metadata);
}

#[test]
fn missing_unreadable_and_invalid_metadata_refuse_loading() {
    let root = TempDir::new().unwrap();
    assert!(matches!(
        MinecraftServer::load(root.path()),
        Err(MinecraftServerError::ReadMetadata { source, .. })
            if source.kind() == io::ErrorKind::NotFound
    ));
    assert!(!root.path().join(".mcpm").exists());
    fs::create_dir_all(root.path().join(".mcpm/server.json")).unwrap();
    assert!(matches!(
        MinecraftServer::load(root.path()),
        Err(MinecraftServerError::ReadMetadata { .. })
    ));
    fs::remove_dir(root.path().join(".mcpm/server.json")).unwrap();
    for invalid in [
        "{".to_owned(),
        json!({"minecraft_version":"1.21.1", "loader":null}).to_string(),
        json!({"version":0, "minecraft_version":"1.21.1", "loader":null}).to_string(),
        json!({"version":2, "minecraft_version":"1.21.1", "loader":null}).to_string(),
        json!({"version":"1", "minecraft_version":"1.21.1", "loader":null}).to_string(),
        json!({"version":1, "loader":null}).to_string(),
        json!({"version":1, "minecraft_version":"1.21.1"}).to_string(),
        json!({"version":1, "minecraft_version":"", "loader":null}).to_string(),
        json!({"version":1, "minecraft_version":"1.21.1", "loader":{"type":"fabric"}}).to_string(),
        json!({"version":1, "minecraft_version":"1.21.1", "loader":{"type":"", "version":"1"}}).to_string(),
        json!({"version":1, "minecraft_version":"1.21.1", "loader":{"type":"fabric", "version":" "}}).to_string(),
    ] {
        fs::write(root.path().join(".mcpm/server.json"), &invalid).unwrap();
        assert!(
            matches!(
                MinecraftServer::load(root.path()),
                Err(MinecraftServerError::ParseMetadata { .. })
            ),
            "accepted invalid metadata: {invalid}"
        );
    }
}

#[test]
fn invalid_registration_does_not_write_metadata_and_roots_must_be_directories() {
    let root = TempDir::new().unwrap();
    let metadata = MinecraftServerMetadata {
        version: 1,
        minecraft_version: "".into(),
        loader: None,
        start: None,
    };
    assert!(matches!(
        MinecraftServer::create(root.path(), metadata),
        Err(MinecraftServerError::ParseMetadata { .. })
    ));
    assert!(!root.path().join(".mcpm").exists());
    let file = root.path().join("file");
    fs::write(&file, "").unwrap();
    assert!(matches!(
        MinecraftServer::load(file),
        Err(MinecraftServerError::NotDirectory { .. })
    ));
}
