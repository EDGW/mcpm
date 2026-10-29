//! Provider I/O, partial failures and shared-runtime connection lifetime regression tests.

use super::*;
use crate::commands::supports::mods::selection::{Scope, SelectionInput};
use mcpm_backend::local::cache::mods::CacheStatus;
use mcpm_backend::local::cache::mods::CacheUnavailableReason;
use mcpm_backend::local::cache::mods::ModsCacheDir;
use mcpm_backend::local::{cache::mods::ModsCacheSelection, runtime::RuntimeEnvironment};
use mcpm_backend::source::FileSummary;
use mcpm_backend::source::SourceId;
use mcpm_backend::source::curseforge::CurseForgeSource;
use mcpm_backend::source::modrinth::ModrinthSource;
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
};

#[test]
fn consecutive_files_reuse_one_live_http_connection_until_the_batch_finishes() {
    let root = tempfile::tempdir().unwrap();
    for name in ["first.jar", "second.jar", "third.jar"] {
        fs::write(root.path().join(name), b"abc").unwrap();
    }
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let environment =
        RuntimeEnvironment::new().with_mods_cache(Some(ModsCacheSelection::Directory {
            path: cache.path().into(),
        }));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "provider connection missing"
                    );
                    thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        for _ in 0..3 {
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(
                    stream.read(&mut byte).unwrap(),
                    1,
                    "shared runtime closed a pooled connection before the next file"
                );
                request.push(byte[0]);
            }
            assert!(request.starts_with(b"GET /version_file/"));
            // An unmatched response still keeps the connection pooled for the next file.
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n"
            )
            .unwrap();
            stream.flush().unwrap();
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    let provider = Arc::new(
        ModrinthSource::with_endpoint(
            SourceId("modrinth".into()),
            client,
            format!("http://{address}/").parse().unwrap(),
        )
        .unwrap(),
    );
    let writing: WriteArgs =
        serde_json::from_value(json!({"metas":["ident"],"parallelism":1})).unwrap();
    let mut completion = Completion::new(&writing, &["modrinth".into()]).unwrap();
    completion.sources = vec![Ok(provider)];
    let selection: SelectionInput = serde_json::from_value(json!({"all":true})).unwrap();
    let mut events = Vec::new();
    let mut emit = |event| {
        events.push(event);
        Ok(())
    };
    super::super::run(
        selection.into_selection().unwrap(),
        Scope::directory(root.path().into()),
        &environment,
        writing.options(Default::default()).unwrap(),
        || Ok(()),
        |event| completion.accept(event, &mut emit),
    )
    .unwrap();
    completion
        .finish(Location::Local(&environment), &mut emit)
        .unwrap();
    server.join().unwrap();
    let Event::Done(summary) = events.last().unwrap() else {
        panic!("missing completion")
    };
    assert_eq!(summary.files, 3);
    assert_eq!(summary.failed_files, 0);
}

#[test]
fn explicit_enrichment_publishes_successes_despite_other_sources_failing_and_identify_never_writes()
{
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.jar");
    fs::write(&input, b"abc").unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let environment =
        RuntimeEnvironment::new().with_mods_cache(Some(ModsCacheSelection::Directory {
            path: cache.path().into(),
        }));
    let summary = FileSummary::read(&input, Default::default()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        for index in 0..2 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "provider request did not arrive"
                        );
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).unwrap() > 0);
            let (status, body) = if index == 0 {
                ("200 OK", json!({"id":"version","project_id":"project","name":"release","version_number":"1","version_type":"release","date_published":"2026-10-22T00:00:00Z","game_versions":[],"loaders":[],"dependencies":[],"files":[{"filename":"additional.jar","size":3,"url":"https://example.com/file.jar","primary":false,"hashes":{"sha1":summary.sha1}}]}).to_string())
            } else {
                ("500 Internal Server Error", "{}".into())
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let modrinth: Arc<dyn mcpm_backend::source::Source> = Arc::new(
        ModrinthSource::with_endpoint(
            SourceId("modrinth".into()),
            client.clone(),
            format!("http://{address}/").parse().unwrap(),
        )
        .unwrap(),
    );
    let curseforge = Arc::new(
        CurseForgeSource::with_endpoint(
            SourceId("curseforge".into()),
            client,
            format!("http://{address}/").parse().unwrap(),
            None,
        )
        .unwrap(),
    );
    let writing: WriteArgs =
        serde_json::from_value(json!({ "metas":["ident"],"parallelism":2})).unwrap();
    let mut completion =
        Completion::new(&writing, &["modrinth".into(), "curseforge".into()]).unwrap();
    completion.sources = vec![Ok(modrinth.clone()), Ok(curseforge)];
    let selection: SelectionInput =
        serde_json::from_value(json!({"queries":[{"kind":"file","value":"input.jar"}]})).unwrap();
    let mut output = Vec::new();
    let mut emit = |event| {
        output.push(event);
        Ok(())
    };
    super::super::run(
        selection.into_selection().unwrap(),
        Scope::directory(root.path().into()),
        &environment,
        writing.options(Default::default()).unwrap(),
        || Ok(()),
        |event| completion.accept(event, &mut emit),
    )
    .unwrap();
    completion
        .finish(Location::Local(&environment), &mut emit)
        .unwrap();
    server.join().unwrap();
    let mut validator = super::super::validation::Validator::default();
    for event in &output {
        validator.accept(event).unwrap();
    }
    validator.finish().unwrap();
    let Event::Done(summary) = output.last().unwrap() else {
        panic!("missing completion")
    };
    assert_eq!(summary.failed_files, 1);
    let id = cache.list_metadata().unwrap()[0].id();
    let reader = &cache;
    assert_eq!(
        reader.mod_metadata(id).status(),
        &CacheStatus::Unavailable(CacheUnavailableReason::MissingDigest)
    );
    assert!(!reader.mod_metadata(id).digest_path().exists());
    let info = reader.ident_info(id);
    assert!(info.get("modrinth").is_some());
    assert!(info.get("curseforge").is_none());
    let saved = fs::read(info.path()).unwrap();
    let modified = fs::metadata(info.path()).unwrap().modified().unwrap();
    // The provider listener is closed: a read-only cache hit still succeeds.
    for selection in [
        Some(ModsCacheSelection::Directory {
            path: cache.path().into(),
        }),
        None,
    ] {
        let mut matched = false;
        let runtime = Runtime::new().unwrap();
        runtime
            .block_on(
                Identification::with_sources(
                    vec![input.clone()],
                    vec![Ok(modrinth.clone())],
                    1,
                    Location::Local(&RuntimeEnvironment::new().with_mods_cache(selection.clone())),
                )
                .unwrap()
                .run(|event| {
                    if let identify::Event::Item {
                        outcome: Outcome::Matched { .. },
                        ..
                    } = event
                    {
                        matched = true;
                    }
                    Ok(())
                }),
            )
            .unwrap();
        assert_eq!(matched, selection.is_some());
        assert_eq!(fs::read(info.path()).unwrap(), saved);
        assert_eq!(
            fs::metadata(info.path()).unwrap().modified().unwrap(),
            modified
        );
    }
}
