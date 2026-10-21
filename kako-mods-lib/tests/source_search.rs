//! HTTP contract checks for native search alternatives, limits, and unrestricted queries.

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use kako_mods_lib::source::{
    SearchQuery, Source, SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource,
};
use serde_json::{Value, json};

#[test]
fn search_filters_use_native_or_groups_and_keep_platform_pagination() {
    for curseforge in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for mode in 0..3 {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "search request timed out");
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
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
                let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
                assert_eq!(
                    params[if curseforge { "searchFilter" } else { "query" }],
                    "机械 & Rails"
                );
                if curseforge {
                    assert_eq!(url.path(), "/mods/search");
                    assert_eq!(params["gameId"], "432");
                    assert_eq!(params["classId"], "6");
                    assert_eq!(params["index"], "20");
                    assert_eq!(params["pageSize"], "10");
                    match mode {
                        0 => {
                            assert_eq!(
                                serde_json::from_str::<Value>(&params["gameVersions"]).unwrap(),
                                json!(["1.20.1", "1.21.1"])
                            );
                            assert_eq!(
                                serde_json::from_str::<Value>(&params["modLoaderTypes"]).unwrap(),
                                json!([1, 6])
                            );
                            assert!(
                                !params.contains_key("gameVersion")
                                    && !params.contains_key("modLoaderType")
                            );
                        }
                        1 => {
                            assert_eq!(params["gameVersion"], "1.20.1");
                            assert_eq!(params["modLoaderType"], "1");
                            assert!(
                                !params.contains_key("gameVersions")
                                    && !params.contains_key("modLoaderTypes")
                            );
                        }
                        _ => assert!(!params.keys().any(|key| {
                            [
                                "gameVersion",
                                "gameVersions",
                                "modLoaderType",
                                "modLoaderTypes",
                            ]
                            .contains(&key.as_str())
                        })),
                    }
                } else {
                    assert_eq!(url.path(), "/search");
                    assert_eq!(params["offset"], "20");
                    assert_eq!(params["limit"], "10");
                    let facets: Value = serde_json::from_str(&params["facets"]).unwrap();
                    assert_eq!(
                        facets,
                        match mode {
                            0 => json!([
                                ["project_type:mod"],
                                ["versions:1.20.1", "versions:1.21.1"],
                                ["categories:forge", "categories:neoforge"]
                            ]),
                            1 => json!([
                                ["project_type:mod"],
                                ["versions:1.20.1"],
                                ["categories:forge"]
                            ]),
                            _ => json!([["project_type:mod"]]),
                        }
                    );
                }
                let body = if curseforge {
                    json!({"data": [], "pagination": {"index": 20, "totalCount": 100}})
                } else {
                    json!({"hits": [], "offset": 20, "total_hits": 100})
                }
                .to_string();
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
            let mut query = SearchQuery {
                query: "机械 & Rails".into(),
                game_versions: vec!["1.20.1".into(), "1.21.1".into()],
                loaders: vec!["forge".into(), "neoforge".into()],
                offset: 20,
                limit: 10,
                ..Default::default()
            };
            for mode in 0..3 {
                let mut invalid = query.clone();
                invalid.loaders = vec!["unknown".into()];
                assert!(
                    source
                        .search(&invalid)
                        .await
                        .unwrap_err()
                        .to_string()
                        .contains("unsupported")
                );
                if curseforge {
                    invalid = query.clone();
                    invalid.game_versions = vec!["1.21.1".into(); 5];
                    assert!(
                        source
                            .search(&invalid)
                            .await
                            .unwrap_err()
                            .to_string()
                            .contains("at most 4")
                    );
                    invalid = query.clone();
                    invalid.loaders = vec!["forge".into(); 6];
                    assert!(
                        source
                            .search(&invalid)
                            .await
                            .unwrap_err()
                            .to_string()
                            .contains("at most 5")
                    );
                }
                let page = source.search(&query).await.unwrap();
                assert_eq!(page.offset, 20);
                assert_eq!(page.total, Some(100));
                if mode == 0 {
                    query.game_versions.truncate(1);
                    query.loaders.truncate(1);
                } else {
                    query.game_versions.clear();
                    query.loaders.clear();
                }
            }
        });
        server.join().unwrap();
    }
}
