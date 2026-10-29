//! Provider HTTP regressions for combined version/loader alternatives before file pagination.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use mcpm_backend::source::{
    FileQuery, ProjectId, ProjectRef, Source, SourceId, curseforge::CurseForgeSource,
    modrinth::ModrinthSource,
};
use serde_json::json;

#[test]
fn multiple_loaders_filter_before_pagination_on_both_providers() {
    let loader_sets = [
        vec!["forge"],
        vec!["fabric"],
        vec!["quilt"],
        vec!["neoforge"],
        vec!["fabric", "neoforge"],
    ];
    for (curseforge, multiple_versions) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let versions: Vec<_> = loader_sets.iter().enumerate().map(|(index, loaders)| {
            let id = index + 1;
            let game_version = if id == 2 { "1.19.2" } else if id == 4 { "1.20.1" } else { "1.21.1" };
            if curseforge {
                json!({
                    "id": id, "modId": 123, "displayName": format!("file-{id}"),
                    "fileName": format!("file-{id}.jar"), "releaseType": 1,
                    "fileLength": 1, "downloadUrl": null, "hashes": [], "dependencies": [],
                    "sortableGameVersions": loaders.iter().map(|loader| json!({
                        "gameVersionName": loader, "gameVersionTypeId": 68441, "gameVersion": ""
                    })).chain(std::iter::once(json!({
                        "gameVersionName": game_version, "gameVersionTypeId": 999, "gameVersion": game_version
                    }))).collect::<Vec<_>>()
                })
            } else {
                json!({
                    "id": format!("v{id}"), "project_id": "123", "name": format!("file-{id}"),
                    "version_number": id.to_string(), "version_type": "release",
                    "date_published": format!("2026-10-{:02}T00:00:00Z", 21-index),
                    "game_versions": [game_version], "loaders": loaders, "dependencies": [],
                    "files": [{"filename": format!("file-{id}.jar"), "size": 1,
                        "url": format!("https://example.com/file-{id}.jar"), "primary": true, "hashes": {}}]
                })
            }
        }).collect();
        let responses = if curseforge {
            vec![
                (
                    "/mods/123/files?index=0&pageSize=50",
                    json!({"data": versions[..3], "pagination": {"index": 0, "totalCount": 5}}),
                ),
                (
                    "/mods/123/files?index=3&pageSize=50",
                    json!({"data": versions[3..], "pagination": {"index": 3, "totalCount": 5}}),
                ),
            ]
        } else {
            vec![("/project/123/version", json!(versions))]
        };
        let server = thread::spawn(move || {
            for (path, response) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "file request timed out");
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
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                assert!(
                    String::from_utf8(request)
                        .unwrap()
                        .starts_with(&format!("GET {path} HTTP/1.1\r\n"))
                );
                let body = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
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
                        SourceId("test".into()),
                        client,
                        endpoint.parse().unwrap(),
                        None,
                    )
                    .unwrap(),
                )
            } else {
                Box::new(
                    ModrinthSource::with_endpoint(
                        SourceId("test".into()),
                        client,
                        endpoint.parse().unwrap(),
                    )
                    .unwrap(),
                )
            };
            let page = source
                .list_files(
                    &ProjectRef {
                        source: SourceId("test".into()),
                        id: ProjectId("123".into()),
                    },
                    &FileQuery {
                        loaders: vec!["fabric".into(), "neoforge".into()],
                        game_versions: if multiple_versions {
                            vec!["1.20.1".into(), "1.21.1".into()]
                        } else {
                            vec![]
                        },
                        offset: 1,
                        limit: 2,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let ids: Vec<_> = page
                .files
                .iter()
                .map(|file| file.reference.id.0.as_str())
                .collect();
            assert_eq!(
                ids,
                match (curseforge, multiple_versions) {
                    (true, false) => vec!["4", "5"],
                    (false, false) => vec!["v4", "v5"],
                    (true, true) => vec!["5"],
                    (false, true) => vec!["v5"],
                }
            );
            assert_eq!(page.total, Some(if multiple_versions { 2 } else { 3 }));
            assert_eq!(page.offset, 1);
        });
        server.join().unwrap();
    }
}
