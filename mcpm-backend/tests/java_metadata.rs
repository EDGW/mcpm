//! Java metadata decoding exercised through Fabric server identity extraction.

use std::{fs, io::Write, path::Path};

use mcpm_backend::local::{mc::server::identify::identify, modloader::BUILTIN_LOADERS};
use zip::{ZipWriter, write::SimpleFileOptions};

fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut archive = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        archive
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
}

#[test]
fn fabric_installer_metadata_uses_java_duplicate_and_escape_semantics() {
    let root = tempfile::tempdir().unwrap();
    archive(&root.path().join("server.jar"), &[
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMain-Class: wrong.Main\r\nmain-class: net.fabricmc.installer.\r\n ServerLauncher\r\n\r\nName: ignored\r\nMain-Class: another.Main\r\n"),
        ("install.properties", b"game-version=old\r\ngame\\u002dversion : 1.21.\\\r\n  1\r\nfabric-loader-version=old\nfabric\\-loader-version=0.16.\\\r 10\r\nunused=caf\xe9\nempty\n=value\n"),
    ]);
    fs::write(root.path().join("run.sh"), "java -jar server.jar\n").unwrap();
    let candidates = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].metadata.minecraft_version, "1.21.1");
    let loader = candidates[0].metadata.loader.as_ref().unwrap();
    assert_eq!(loader.loader_type, "fabric");
    assert_eq!(loader.version, "0.16.10");
}

#[test]
fn fabric_game_selection_uses_the_last_decoded_properties_path() {
    let root = tempfile::tempdir().unwrap();
    archive(
        &root.path().join("game server.jar"),
        &[("version.json", br#"{"id":"1.21.1"}"#)],
    );
    fs::write(
        root.path().join("fabric-server-launcher.properties"),
        b"serverJar=wrong.jar\nserverJar=game\\u0020server.\\\n  jar\nopaque\\ key : =value  \n",
    )
    .unwrap();
    fs::write(root.path().join("run.sh"), "java -cp libraries/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar net.fabricmc.loader.impl.launch.server.FabricServerLauncher\n").unwrap();
    let candidates = identify(root.path(), "run.sh", &BUILTIN_LOADERS).unwrap();
    assert_eq!(candidates[0].metadata.minecraft_version, "1.21.1");
    assert_eq!(
        candidates[0].metadata.loader.as_ref().unwrap().version,
        "0.16.10"
    );
}
