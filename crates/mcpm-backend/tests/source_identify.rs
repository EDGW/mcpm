//! Provider response verification and positive-only caching when remote matches are ambiguous.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use mcpm_backend::{
    local::cache::{
        CacheLinkPreference,
        mods::{MetadataRefresh, ModsCacheDir, ident::RefreshStatus},
    },
    source::{
        FileSummary, Source, SourceId, SummaryRequirements, curseforge::CurseForgeSource,
        modrinth::ModrinthSource,
    },
};
use serde_json::{Value, json};

#[test]
fn ambiguous_verified_candidates_are_unmatched_and_preserve_positive_cache() {
    for curseforge in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("input.jar");
        std::fs::write(&path, b"abc").unwrap();
        let summary = FileSummary::read(&path, SummaryRequirements { murmur2: true }).unwrap();
        let file = |name: &str, id, size| {
            if curseforge {
                json!({"id": 7, "file": {
                    "id": id, "modId": 7, "fileName": name, "fileLength": size,
                    "fileFingerprint": summary.murmur2.unwrap(),
                    "hashes": [{"algo": 1, "value": summary.sha1}]
                }})
            } else {
                json!({
                    "filename": name, "size": size, "url": "https://example.com/file.jar",
                    "primary": false, "hashes": {"sha1": summary.sha1}
                })
            }
        };
        let response = |files: Vec<Value>| {
            if curseforge {
                json!({"data": {"exactMatches": files}})
            } else {
                json!({
                    "id": "version", "project_id": "project", "name": "Version",
                    "version_number": "1", "version_type": "release",
                    "date_published": "2026-01-01T00:00:00Z", "game_versions": [],
                    "loaders": [], "files": files, "dependencies": []
                })
            }
        };
        let valid_file = file("actual.jar", 10, summary.size);
        let valid = response(vec![valid_file.clone()]);
        let ambiguous = response(vec![
            valid_file.clone(),
            file("other.jar", 11, summary.size),
        ]);
        let duplicate = response(vec![valid_file.clone(), valid_file.clone()]);
        let filtered = response(vec![
            file("wrong-size.jar", 12, summary.size + 1),
            valid_file,
        ]);
        let malformed = json!({"invalid": true});
        let responses = vec![
            valid.clone(),
            ambiguous.clone(),
            duplicate,
            filtered,
            malformed,
            ambiguous.clone(),
            ambiguous.clone(),
            valid,
            ambiguous,
        ];
        let (endpoint, server) = serve(responses);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let source: Box<dyn Source> = if curseforge {
                Box::new(
                    CurseForgeSource::with_endpoint(
                        SourceId("provider".into()),
                        client,
                        endpoint.parse().unwrap(),
                        Some("test-key"),
                    )
                    .unwrap(),
                )
            } else {
                Box::new(
                    ModrinthSource::with_endpoint(
                        SourceId("provider".into()),
                        client,
                        endpoint.parse().unwrap(),
                    )
                    .unwrap(),
                )
            };
            let matched = source.identify_summary(&summary).await.unwrap().unwrap();
            assert_eq!(matched.filename, "actual.jar");
            assert!(source.identify_summary(&summary).await.unwrap().is_none());
            assert_eq!(
                source.identify_summary(&summary).await.unwrap().is_some(),
                curseforge,
                "identical CurseForge records describe one candidate"
            );
            assert_eq!(
                source.identify_summary(&summary).await.unwrap(),
                Some(matched)
            );
            assert!(source.identify_summary(&summary).await.is_err());

            let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
            let id = cache.import(&path, CacheLinkPreference::Copy).unwrap();
            for _ in 0..2 {
                let outcome = cache
                    .refresh_ident_info_with_outcome(
                        id,
                        source.as_ref(),
                        MetadataRefresh::MissingOrOutdated,
                    )
                    .await
                    .unwrap();
                assert_eq!(outcome.status, RefreshStatus::Unmatched);
                assert_eq!(outcome.info.matches().count(), 0);
                assert!(cache.inspect(&id).unwrap().metadata.is_empty());
            }
            let outcome = cache
                .refresh_ident_info_with_outcome(
                    id,
                    source.as_ref(),
                    MetadataRefresh::MissingOrOutdated,
                )
                .await
                .unwrap();
            assert_eq!(outcome.status, RefreshStatus::Matched);
            let previous: Vec<_> = outcome
                .info
                .matches()
                .map(|(id, file)| (id.to_owned(), file.clone()))
                .collect();
            let outcome = cache
                .refresh_ident_info_with_outcome(id, source.as_ref(), MetadataRefresh::Force)
                .await
                .unwrap();
            assert_eq!(outcome.status, RefreshStatus::Unmatched);
            assert_eq!(
                outcome
                    .info
                    .matches()
                    .map(|(id, file)| (id.to_owned(), file.clone()))
                    .collect::<Vec<_>>(),
                previous
            );
        });
        server.join().unwrap();
    }
}

fn serve(responses: Vec<Value>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/api/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "identification request timed out"
                        );
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let header_end = loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + length {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (endpoint, server)
}
