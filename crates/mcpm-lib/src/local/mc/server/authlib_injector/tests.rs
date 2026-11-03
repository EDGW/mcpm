//! Managed installation state transitions, fixed-release verification, failed refresh and lease cleanup.

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use super::*;
use crate::download::{
    DownloadOptions,
    sources::{MappedSource, UrlMapping},
};

#[test]
fn launch_preflights_before_downloading_and_returns_a_plan_without_a_lease() {
    use crate::local::mc::server::{
        MinecraftServer, MinecraftServerMetadata, config::ConfigurationOverrides, launch,
    };
    let server = Server::new();
    let root = tempfile::tempdir().unwrap();
    fn assert_send(_: impl Send) {}
    assert_send(launch::prepare(root.path(), || server.downloader()));
    MinecraftServer::create(
        root.path(),
        MinecraftServerMetadata {
            version: 3,
            minecraft_version: "1.21.1".into(),
            loader: None,
            start: None,
        },
    )
    .unwrap();
    fs::create_dir(root.path().join("work")).unwrap();
    fs::write(
        root.path().join("launch.toml"),
        "[process]\nworkingDirectory='work'\n[entry]\ntype='class'\nmainClass='example.Main'\n",
    )
    .unwrap();
    fs::write(root.path().join("conf.toml"), "[java]\n").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut invalid = ConfigurationOverrides::default();
        invalid.set("java.executable", "./missing-java").unwrap();
        invalid
            .set("authlib-injector.apiUrl", "https://example.com/")
            .unwrap();
        assert!(
            launch::prepare_with_overrides(root.path(), &invalid, None, || panic!(
                "invalid local conditions must precede downloading"
            ))
            .await
            .is_err()
        );
        assert!(!root.path().join(".mcpm/authlib-injector").exists());

        let mut raw_agents = vec![
            "-javaagent:../.mcpm/./authlib-injector/authlib-injector.jar=https://other.example/",
        ];
        if cfg!(windows) {
            raw_agents.push(
                "-javaagent:../.mcpm/AUTHLIB-INJECTOR/AUTHLIB-INJECTOR.JAR=https://other.example/",
            );
        }
        for raw in raw_agents {
            let mut duplicate = ConfigurationOverrides::default();
            duplicate
                .set(
                    "java.executable",
                    std::env::current_exe().unwrap().to_str().unwrap(),
                )
                .unwrap();
            duplicate
                .set("authlib-injector.apiUrl", "https://example.com/")
                .unwrap();
            duplicate.set("jvm.extraArguments", vec![raw]).unwrap();
            assert!(
                launch::prepare_with_overrides(root.path(), &duplicate, None, || panic!(
                    "missing managed path must not hide a duplicate agent"
                ))
                .await
                .is_err()
            );
            assert!(!root.path().join(".mcpm/authlib-injector").exists());
        }

        let mut changes = ConfigurationOverrides::default();
        changes
            .set(
                "java.executable",
                std::env::current_exe().unwrap().to_str().unwrap(),
            )
            .unwrap();
        changes
            .set("authlib-injector.apiUrl", "https://example.com/")
            .unwrap();
        let plan =
            launch::prepare_with_overrides(root.path(), &changes, None, || server.downloader())
                .await
                .unwrap();
        let managed = fs::canonicalize(root.path())
            .unwrap()
            .join(".mcpm/authlib-injector/authlib-injector.jar");
        assert!(plan.arguments().contains(&format!(
            "-javaagent:{}=https://example.com/",
            managed.display()
        )));
        assert_eq!(
            plan.directory(),
            fs::canonicalize(root.path().join("work")).unwrap()
        );
        let _lease = storage::Storage::new(root.path()).lock().unwrap();
        drop(_lease);
        launch::prepare_with_overrides(root.path(), &changes, Some(JarPolicy::Local), || {
            panic!("valid installed JAR must not download")
        })
        .await
        .unwrap();

        changes
            .set(
                "jvm.extraArguments",
                vec![format!(
                    "-javaagent:{}=https://other.example/",
                    managed.display()
                )],
            )
            .unwrap();
        assert!(
            launch::prepare_with_overrides(
                root.path(),
                &changes,
                Some(JarPolicy::Refresh),
                || panic!("duplicate agents must fail before downloading")
            )
            .await
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("conf.toml")).unwrap(),
            "[java]\n"
        );
    });
}

#[test]
fn managed_policy_repairs_missing_and_corrupt_pairs_without_checking_valid_versions() {
    let server = Server::new();
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let jar = prepare(root.path(), JarPolicy::Auto, || server.downloader())
            .await
            .unwrap();
        let meta = jar.with_file_name("meta.json");
        assert_eq!(fs::read(&jar).unwrap(), b"agent");
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        for policy in [JarPolicy::Auto, JarPolicy::Local] {
            prepare(root.path(), policy, || {
                panic!("valid local pair must not create a downloader")
            })
            .await
            .unwrap();
        }
        for corruption in 0..4 {
            match corruption {
                0 => fs::remove_file(&meta).unwrap(),
                1 => fs::write(&meta, b"corrupt JSON").unwrap(),
                2 => fs::write(&jar, b"tampered").unwrap(),
                _ => fs::remove_file(&jar).unwrap(),
            }
            assert!(
                prepare(root.path(), JarPolicy::Local, || panic!(
                    "local policy must not download"
                ))
                .await
                .is_err()
            );
            prepare(root.path(), JarPolicy::Auto, || server.downloader())
                .await
                .unwrap();
            assert_eq!(fs::read(&jar).unwrap(), b"agent");
        }
        assert_eq!(server.requests.lock().unwrap().len(), 10);
        prepare(root.path(), JarPolicy::Refresh, || server.downloader())
            .await
            .unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 12);

        fs::write(&meta, br#"{"formatVersion":2}"#).unwrap();
        for policy in [JarPolicy::Auto, JarPolicy::Refresh] {
            let error = prepare(root.path(), policy, || {
                panic!("unknown format must not be replaced")
            })
            .await
            .unwrap_err();
            assert!(error.is::<metadata::UnsupportedFormat>());
        }
        assert_eq!(fs::read(&meta).unwrap(), br#"{"formatVersion":2}"#);
    });
}

#[test]
fn failed_refresh_preserves_pair_and_releases_preparation_lock() {
    let server = Server::new();
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let jar = prepare(root.path(), JarPolicy::Auto, || server.downloader())
            .await
            .unwrap();
        let meta = jar.with_file_name("meta.json");
        let original_meta = fs::read(&meta).unwrap();
        server.corrupt.store(true, Ordering::SeqCst);
        assert!(
            prepare(root.path(), JarPolicy::Refresh, || server.downloader())
                .await
                .unwrap_err()
                .is::<crate::download::ChecksumMismatch>()
        );
        assert_eq!(fs::read(&jar).unwrap(), b"agent");
        assert_eq!(fs::read(&meta).unwrap(), original_meta);
        assert_eq!(fs::read_dir(jar.parent().unwrap()).unwrap().count(), 3);

        let storage = storage::Storage::new(root.path());
        let lease = storage.lock().unwrap();
        assert!(
            prepare(root.path(), JarPolicy::Auto, || panic!(
                "busy must not download"
            ))
            .await
            .is_err()
        );
        drop(lease);
        prepare(root.path(), JarPolicy::Local, || {
            panic!("lock must be released")
        })
        .await
        .unwrap();

        fs::remove_file(&meta).unwrap();
        fs::create_dir(&meta).unwrap();
        assert!(
            prepare(root.path(), JarPolicy::Auto, || panic!(
                "directory is not repairable corruption"
            ))
            .await
            .is_err()
        );
        assert!(meta.is_dir());
    });
}

#[test]
fn cancelling_download_preserves_current_pair_and_discards_staging() {
    let server = Server::new();
    let root = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let original = runtime.block_on(async {
        let jar = prepare(root.path(), JarPolicy::Auto, || server.downloader())
            .await
            .unwrap();
        let meta = jar.with_file_name("meta.json");
        let original = fs::read(&meta).unwrap();
        server.stall.store(true, Ordering::SeqCst);
        let mut download =
            std::pin::pin!(prepare(root.path(), JarPolicy::Refresh, || server.downloader()));
        // Poll until the fixture has received the JAR request, not just until a timer expires.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        std::future::poll_fn(|cx| {
            use std::{future::Future, task::Poll};
            assert!(download.as_mut().poll(cx).is_pending());
            if server.requests.lock().unwrap().len() == 4 {
                return Poll::Ready(());
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "fixture did not receive the download"
            );
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
        original
    }); // Dropping the preparation future must release the lock and remove staged files.
    let directory = root.path().join(".mcpm/authlib-injector");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
    assert_eq!(
        fs::read(directory.join("authlib-injector.jar")).unwrap(),
        b"agent"
    );
    assert_eq!(fs::read(directory.join("meta.json")).unwrap(), original);
    runtime
        .block_on(prepare(root.path(), JarPolicy::Local, || {
            panic!("cancelled refresh must leave a usable pair")
        }))
        .unwrap();
}

/// Local protocol fixture; request paths reveal accidental latest re-resolution or network checks.
struct Server {
    address: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    corrupt: Arc<AtomicBool>,
    stall: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let corrupt = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let stall = Arc::new(AtomicBool::new(false));
        let (seen, broken, stopping, stalled) = (
            requests.clone(),
            corrupt.clone(),
            stop.clone(),
            stall.clone(),
        );
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let Ok(count) = stream.read(&mut buffer) else {
                        break;
                    };
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                let Some(path) = request.split_whitespace().nth(1) else {
                    continue;
                };
                seen.lock().unwrap().push(path.to_owned());
                let body = if path == "/artifact/latest.json" {
                    br#"{"build_number":56,"version":"fixture","release_time":"2026-07-11T17:14:13Z","download_url":"https://authlib-injector.yushi.moe/artifact/56/agent.jar","checksums":{"sha256":"d4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824"}}"#.to_vec()
                } else if broken.load(Ordering::SeqCst) {
                    b"broken".to_vec()
                } else {
                    b"agent".to_vec()
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                while path != "/artifact/latest.json"
                    && stalled.load(Ordering::SeqCst)
                    && !stopping.load(Ordering::SeqCst)
                {
                    thread::sleep(Duration::from_millis(5));
                }
                let _ = stream.write_all(&body);
            }
        });
        Self {
            address,
            requests,
            corrupt,
            stall,
            stop,
            thread: Some(thread),
        }
    }

    fn downloader(&self) -> Result<Downloader> {
        let source = MappedSource::new(
            "fixture",
            vec![UrlMapping::new(
                "https://authlib-injector.yushi.moe/".parse()?,
                format!("http://{}/", self.address).parse()?,
            )?],
        )?;
        Downloader::new(vec![Arc::new(source)], DownloadOptions::default())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}
