//! HTTP contracts for source-native Minecraft catalogs, normalization and upstream failures.

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use mcpm_backend::source::{
    Source, SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource,
};
use serde_json::json;

#[test]
fn catalogs_use_configured_endpoints_and_preserve_order_without_duplicate_versions() {
    for curseforge in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/proxy/v1/", listener.local_addr().unwrap());
        let field = if curseforge {
            "versionString"
        } else {
            "version"
        };
        let wrap = |values| {
            if curseforge {
                json!({"data": values})
            } else {
                values
            }
        };
        let responses = vec![
            (
                "200 OK",
                wrap(json!([
                    {field: "1.21.1", "extra_metadata": true},
                    {field: "1.20.1"}, {field: "1.21.1"}, {field: "24w14a"}
                ]))
                .to_string(),
            ),
            ("200 OK", wrap(json!([])).to_string()),
            (
                "200 OK",
                wrap(json!([{"wrong_field": "1.21.1"}])).to_string(),
            ),
            ("200 OK", wrap(json!([{field: " "}])).to_string()),
            ("503 Service Unavailable", "{}".into()),
        ];
        let server = thread::spawn(move || {
            for (status, body) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "catalog request timed out");
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
                let path = if curseforge {
                    "minecraft/version"
                } else {
                    "tag/game_version"
                };
                assert!(request.starts_with(&format!("GET /proxy/v1/{path} HTTP/1.1\r\n")));
                if curseforge {
                    assert!(
                        request
                            .to_ascii_lowercase()
                            .contains("x-api-key: catalog-test-key\r\n")
                    );
                }
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
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
                        SourceId("catalog".into()),
                        client,
                        endpoint.parse().unwrap(),
                        Some("catalog-test-key"),
                    )
                    .unwrap(),
                )
            } else {
                Box::new(
                    ModrinthSource::with_endpoint(
                        SourceId("catalog".into()),
                        client,
                        endpoint.parse().unwrap(),
                    )
                    .unwrap(),
                )
            };
            assert_eq!(
                source.game_versions().await.unwrap(),
                ["1.21.1", "1.20.1", "24w14a"]
            );
            assert!(source.game_versions().await.unwrap().is_empty());
            let provider = if curseforge { "CurseForge" } else { "Modrinth" };
            for expected in ["invalid", "empty", "HTTP error"] {
                let error = source.game_versions().await.unwrap_err().to_string();
                assert!(
                    error.contains(provider) && error.contains(expected),
                    "{error}"
                );
            }
        });
        server.join().unwrap();
    }
}
