//! Consumer-visible boundaries of typed operations, workspace precedence, and host preparation.

use mcpm_backend::{
    operations::{
        self,
        server::{
            info::{Info, InfoInput},
            init::{
                Init, InitEvent, Initialize, LocalImport, Recognize, RecognizeEvent,
                RecognizeServer,
                prepare_remote::{PrepareRemote, PrepareRemoteInput},
            },
        },
        workspace::{RegisterRemote, RegisterRemoteInput},
    },
    workspace,
};
use mcpm_exec::{
    messaging::Session,
    operation::{self, Connection, Operation, Target},
};
use mcpm_lib::local::mc::server::{MinecraftServer, MinecraftServerMetadata};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::JoinSet;

fn identity(version: &str) -> MinecraftServerMetadata {
    serde_json::from_value(
        serde_json::json!({"version":3,"minecraft_version":version,"loader":null}),
    )
    .unwrap()
}
fn input(path: &Path, version: &str) -> Initialize {
    Initialize {
        path: path.into(),
        metadata: identity(version),
        force: false,
        no_git: true,
        git: false,
        local: Some(LocalImport {
            configuration: None,
            replace_conf: false,
        }),
    }
}
async fn client() -> Session {
    let (a, b) = tokio::io::duplex(1024);
    let mut server = Session::new(b);
    tokio::spawn(async move {
        operation::serve(&mut server, Arc::new(operations::registry().unwrap()))
            .await
            .unwrap();
    });
    Session::new(a)
}
fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".mcpm" {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

#[tokio::test]
async fn recognition_is_read_only_and_transports_the_selected_configuration_draft() {
    let root = tempfile::tempdir().unwrap();
    copy(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../mcpm/tests/fixtures/server-init/vanilla"),
        root.path(),
    );
    let session = client().await;
    let mut stream = operation::call::<RecognizeServer>(
        &session,
        &Recognize {
            path: root.path().into(),
            start_script: Some("run.sh".into()),
            minecraft_version: None,
            loader: None,
            loader_version: None,
        },
    )
    .await
    .unwrap();
    let mut candidates = None;
    while let Some(event) = stream.next().await {
        if let RecognizeEvent::Candidates(result) = event.unwrap() {
            candidates = Some(result);
        }
    }
    assert!(!root.path().join(".mcpm").exists());
    assert!(!root.path().join("mods").exists());
    let candidate = candidates.unwrap().remove(0);
    let mut stream = operation::call::<Init>(
        &session,
        &Initialize {
            path: root.path().into(),
            metadata: candidate.metadata,
            force: false,
            no_git: true,
            git: false,
            local: Some(LocalImport {
                configuration: candidate.configuration,
                replace_conf: false,
            }),
        },
    )
    .await
    .unwrap();
    let mut initialized = None;
    while let Some(event) = stream.next().await {
        if let InitEvent::Initialized(result) = event.unwrap() {
            initialized = Some(result);
        }
    }
    assert!(initialized.unwrap().publication.unwrap().entry_configured);
    let server = MinecraftServer::inspect(root.path()).unwrap();
    assert_eq!(server.metadata().minecraft_version, "1.20.1");
    assert!(root.path().join("launch.toml").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_remote_initializers_do_not_overwrite_the_winning_registration() {
    let root = tempfile::tempdir().unwrap();
    let session = Arc::new(client().await);
    let mut attempts = JoinSet::new();
    for index in 0..8 {
        let session = session.clone();
        let input = input(root.path(), &format!("1.21.{index}"));
        attempts.spawn(async move {
            let mut stream = operation::call::<Init>(&session, &input).await?;
            let mut version = None;
            while let Some(event) = stream.next().await {
                if let InitEvent::Initialized(result) = event? {
                    version = Some(result.server.minecraft_version);
                }
            }
            anyhow::Ok(version.unwrap())
        });
    }
    let mut winners = Vec::new();
    while let Some(result) = attempts.join_next().await {
        if let Ok(version) = result.unwrap() {
            winners.push(version);
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(
        MinecraftServer::inspect(root.path())
            .unwrap()
            .metadata()
            .minecraft_version,
        winners[0]
    );
}

#[tokio::test]
async fn explicit_ssh_precedence_does_not_read_a_broken_local_workspace_manifest() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".mcpm")).unwrap();
    fs::write(root.path().join(".mcpm/mcpm.json"), "invalid").unwrap();
    let connection = Connection {
        ssh: "ssh explicit-host".into(),
        mcpmd: None,
    };
    let (target, path) = workspace::resolve(root.path().into(), Some(connection.clone()))
        .await
        .unwrap();
    assert_eq!(target, Target::Remote(connection));
    assert_eq!(path, root.path());
    assert!(workspace::resolve(root.path().into(), None).await.is_err());
}

#[tokio::test]
async fn remote_stub_preparation_owns_host_git_and_shallow_registration_never_creates_local_git() {
    let host = tempfile::tempdir().unwrap();
    let stub = tempfile::tempdir().unwrap();
    MinecraftServer::create(host.path(), identity("1.21.1")).unwrap();
    let session = client().await;
    let initialized = operation::call::<PrepareRemote>(
        &session,
        &PrepareRemoteInput {
            path: host.path().into(),
            no_git: false,
            git: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(initialized.server.minecraft_version, "1.21.1");
    assert!(host.path().join(".git").is_dir());
    RegisterRemote::run(RegisterRemoteInput {
        path: stub.path().into(),
        connection: Connection {
            ssh: "ssh host".into(),
            mcpmd: None,
        },
        remote_path: host.path().display().to_string(),
        force: false,
    })
    .await
    .unwrap();
    assert!(!stub.path().join(".git").exists());
    assert_eq!(
        workspace::inspect(stub.path())
            .unwrap()
            .remote_path
            .as_deref(),
        host.path().to_str()
    );
    let original = fs::read(stub.path().join(".mcpm/server.json")).unwrap();
    assert!(
        Info::run(InfoInput {
            path: stub.path().into()
        })
        .await
        .is_err()
    );
    assert_eq!(
        fs::read(stub.path().join(".mcpm/server.json")).unwrap(),
        original
    );
}

#[tokio::test]
async fn metadata_transport_keeps_foreign_argv_and_inspects_configuration_without_loading_it() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("server ' $()");
    fs::create_dir_all(root.join(".mcpm")).unwrap();
    let metadata = serde_json::json!({
        "version":1,"minecraft_version":"1.21.1",
        "loader":{"type":"fabric","version":"0.16.10"},
        "start":{"script":"run.bat","executable":"C:\\Program Files\\Java\\bin\\java.exe",
            "arguments":["", "-Dmessage=a \"quoted\" value", "C:\\server files\\", "-jar", "server.jar", "$(touch escaped)"],
            "windows":true}
    });
    let bytes = serde_json::to_vec(&metadata).unwrap();
    fs::write(root.join(".mcpm/server.json"), &bytes).unwrap();
    fs::write(
        root.join("conf.toml"),
        "invalid TOML; info only inspects presence",
    )
    .unwrap();
    let session = client().await;
    let info = operation::call::<Info>(&session, &InfoInput { path: root.clone() })
        .await
        .unwrap();
    assert_eq!(info.path, root.display().to_string());
    assert_eq!(info.loader.unwrap().id, "fabric");
    let start = info.start.unwrap();
    assert!(start.windows);
    assert_eq!(start.executable, metadata["start"]["executable"]);
    assert_eq!(
        serde_json::to_value(start.arguments).unwrap(),
        metadata["start"]["arguments"]
    );
    let configuration = info.configuration.unwrap();
    assert!(configuration.conf);
    assert!(!configuration.launch);
    assert_eq!(fs::read(root.join(".mcpm/server.json")).unwrap(), bytes);
    assert!(!root.join("escaped").exists());
}

#[tokio::test]
async fn replacing_a_stub_requires_force_and_force_never_converts_a_local_server() {
    let stub = tempfile::tempdir().unwrap();
    let connection = Connection {
        ssh: "ssh first".into(),
        mcpmd: None,
    };
    RegisterRemote::run(RegisterRemoteInput {
        path: stub.path().into(),
        connection: connection.clone(),
        remote_path: "/first".into(),
        force: false,
    })
    .await
    .unwrap();
    let original = fs::read(stub.path().join(".mcpm/server.json")).unwrap();
    let replacement = Connection {
        ssh: "ssh replacement".into(),
        mcpmd: Some("/opt/mcpmd".into()),
    };
    assert!(
        RegisterRemote::run(RegisterRemoteInput {
            path: stub.path().into(),
            connection: replacement.clone(),
            remote_path: "/replacement".into(),
            force: false
        })
        .await
        .is_err()
    );
    assert_eq!(
        fs::read(stub.path().join(".mcpm/server.json")).unwrap(),
        original
    );
    RegisterRemote::run(RegisterRemoteInput {
        path: stub.path().into(),
        connection: replacement.clone(),
        remote_path: "/replacement".into(),
        force: true,
    })
    .await
    .unwrap();
    let info = workspace::inspect(stub.path()).unwrap();
    assert_eq!(info.connection, Some(replacement));
    assert_eq!(info.remote_path.as_deref(), Some("/replacement"));

    let local = tempfile::tempdir().unwrap();
    MinecraftServer::create(local.path(), identity("1.21.1")).unwrap();
    let original = fs::read(local.path().join(".mcpm/server.json")).unwrap();
    assert!(
        RegisterRemote::run(RegisterRemoteInput {
            path: local.path().into(),
            connection,
            remote_path: "/first".into(),
            force: true
        })
        .await
        .is_err()
    );
    assert_eq!(
        workspace::kind(local.path()).unwrap(),
        workspace::Kind::Local
    );
    assert_eq!(
        fs::read(local.path().join(".mcpm/server.json")).unwrap(),
        original
    );
}
