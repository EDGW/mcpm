//! Local HTTP regression tests for lookup ordering, concurrency, deduplication and failures.

use super::*;
use mcpm_backend::local::{cache::mods::ModsCacheSelection, runtime::RuntimeEnvironment};
use mcpm_backend::source::Source;
use mcpm_backend::source::{SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource};
use std::sync::{Arc, mpsc::channel};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

#[test]
fn lookups_are_sequential_share_duplicate_files_and_continue_after_failure() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.jar");
    let second = directory.path().join("second.jar");
    std::fs::write(&first, b"first archive").unwrap();
    std::fs::write(&second, b"second archive").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..4 {
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
            requests.push(read_request(&mut stream));
            // No second request may start while the first is waiting on its response.
            thread::sleep(Duration::from_millis(20));
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
            );
            let (status, body) = match index {
                0 => ("500 Internal Server Error", "{}"),
                2 => ("404 Not Found", "{}"),
                _ => ("200 OK", r#"{"data":{"exactMatches":[]}}"#),
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (sender, receiver) = channel();
    runtime.block_on(async {
        let sources = sources_for(address);
        identify(vec![first.clone(), second, first], &sources, &sender, 1).await;
    });
    let updates: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        updates
            .iter()
            .filter(|event| matches!(event, Event::Cache { .. }))
            .count(),
        2
    );
    assert!(matches!(updates.last(), Some(Event::Done { failed: 2 })));
    let items: Vec<_> = updates
        .iter()
        .filter_map(|event| match event {
            Event::Item {
                indices,
                source,
                outcome,
            } => Some((indices, source, outcome)),
            _ => None,
        })
        .collect();
    for (index, (indices, source, outcome)) in items.into_iter().enumerate() {
        assert_eq!(*source, index % 2);
        assert_eq!(*indices, if index < 2 { vec![0, 2] } else { vec![1] });
        if index == 0 {
            assert!(matches!(outcome, Outcome::Failed { remote: false, .. }));
        } else {
            assert!(matches!(outcome, Outcome::Unmatched));
        }
    }
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /version_file/"));
    assert!(requests[1].starts_with("POST /fingerprints/432 "));
    assert!(requests[2].starts_with("GET /version_file/"));
    assert_ne!(requests[0], requests[2]);
    assert!(requests[3].starts_with("POST /fingerprints/432 "));
}

#[test]
fn configured_parallelism_bounds_active_requests_and_fills_available_slots() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let paths = (0..4)
        .map(|index| {
            let path = directory.path().join(format!("{index}.jar"));
            std::fs::write(&path, format!("archive {index}")).unwrap();
            path
        })
        .collect();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let server_peak = peak.clone();
    let server = thread::spawn(move || {
        let mut handlers = Vec::new();
        for _ in 0..8 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "lookup stalled");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let active = active.clone();
            let peak = server_peak.clone();
            handlers.push(thread::spawn(move || {
                let request = read_request(&mut stream);
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(80));
                active.fetch_sub(1, Ordering::SeqCst);
                let (status, body) = if request.starts_with("GET ") {
                    ("404 Not Found", "{}")
                } else {
                    ("200 OK", r#"{"data":{"exactMatches":[]}}"#)
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }));
        }
        for handler in handlers {
            handler.join().unwrap();
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (sender, receiver) = channel();
    runtime.block_on(async {
        identify(paths, &sources_for(address), &sender, 3).await;
    });
    server.join().unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 3);
    let updates: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        updates
            .iter()
            .filter(|event| matches!(event, Event::Cache { .. }))
            .count(),
        4
    );
    assert_eq!(
        updates
            .iter()
            .filter(|update| matches!(
                update,
                Event::Item {
                    outcome: Outcome::Unmatched,
                    ..
                }
            ))
            .count(),
        8
    );
}

fn read_request(stream: &mut TcpStream) -> String {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let count = stream.read(&mut buffer).unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned()
}

fn sources_for(address: SocketAddr) -> [Result<Arc<dyn Source>, String>; 2] {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    [
        Ok(Arc::new(
            ModrinthSource::with_endpoint(
                SourceId("modrinth".into()),
                client.clone(),
                format!("http://{address}/").parse().unwrap(),
            )
            .unwrap(),
        )),
        Ok(Arc::new(
            CurseForgeSource::with_endpoint(
                SourceId("curseforge".into()),
                client,
                format!("http://{address}/").parse().unwrap(),
                None,
            )
            .unwrap(),
        )),
    ]
}

async fn identify(
    paths: Vec<PathBuf>,
    sources: &[Result<Arc<dyn Source>, String>; 2],
    sender: &std::sync::mpsc::Sender<Event>,
    parallelism: usize,
) {
    let sources = sources
        .iter()
        .map(|source| {
            source.clone().map_err(|message| ErrorReport {
                message,
                causes: Vec::new(),
            })
        })
        .collect();
    Identification::with_sources(
        paths,
        sources,
        parallelism,
        Location::Local(&RuntimeEnvironment::new().with_mods_cache(None)),
    )
    .unwrap()
    .run_checked(
        || Ok(()),
        |event| {
            sender.send(event)?;
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[test]
fn cancellation_interrupts_a_provider_wait_without_finishing_the_batch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("waiting.jar");
    std::fs::write(&path, b"archive").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let server_cancelled = cancelled.clone();
    let (release, waiting) = channel();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "lookup did not reach provider");
                    thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("{error}"),
            }
        };
        read_request(&mut stream);
        server_cancelled.store(true, Ordering::Release);
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut done = false;
    let result = runtime.block_on(async {
        let sources = vec![
            sources_for(address)[0]
                .clone()
                .map_err(|message| ErrorReport {
                    message,
                    causes: Vec::new(),
                }),
        ];
        tokio::time::timeout(
            Duration::from_secs(2),
            Identification::with_sources(
                vec![path],
                sources,
                1,
                Location::Local(&RuntimeEnvironment::new().with_mods_cache(None)),
            )
            .unwrap()
            .run_checked(
                || {
                    anyhow::ensure!(
                        !cancelled.load(Ordering::Acquire),
                        "cancelled while waiting"
                    );
                    Ok(())
                },
                |event| {
                    done |= matches!(event, Event::Done { .. });
                    Ok(())
                },
            ),
        )
        .await
    });
    let _ = release.send(());
    server.join().unwrap();
    assert!(
        result
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled while waiting")
    );
    assert!(!done);
}

#[cfg(unix)]
#[test]
fn remote_summaries_drive_local_provider_requests_without_reading_remote_paths() {
    use crate::remote::client::Session;
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d";
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "local provider request did not arrive"
                        );
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let request = read_request(&mut stream);
            let body = if request.starts_with("GET ") {
                json!({"id":"version","project_id":"project","name":"release","version_number":"1","version_type":"release","date_published":"2026-10-22T00:00:00Z","game_versions":[],"loaders":[],"dependencies":[],"files":[{"filename":"additional.jar","size":3,"url":"https://example.com/additional.jar","primary":false,"hashes":{"sha1":sha1}}]})
            } else {
                json!({"data":{"exactMatches":[{"id":123,"file":{"id":456,"modId":123,"fileName":"matched.jar","fileLength":3,"fileFingerprint":123456,"hashes":[{"algo":1,"value":sha1}]}}]}})
            }.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            requests.push(request);
        }
        requests
    });
    let request_path = root.path().join("request");
    let reply = |status: &str, contents: serde_json::Value| {
        shlex::try_quote(&format!(
            "{}\n",
            json!({"guid":"GUID","status":status,"contents":contents})
        ))
        .unwrap()
        .replace("GUID", "%s")
    };
    let script = format!(
        r#"
printf '%s\n' '{{"guid":"00000000-0000-0000-0000-000000000000","status":"ready","contents":{{"version":4,"commands":["mod.source.identify"],"max_concurrency":8}}}}'
IFS= read -r logs
log_guid=$(printf '%s' "$logs" | cut -d '"' -f6)
printf {} "$log_guid"
IFS= read -r request
printf '%s' "$request" > {}
guid=$(printf '%s' "$request" | cut -d '"' -f4)
printf {} "$guid"
printf {} "$guid"
printf {} "$guid"
printf {} "$guid"
cat >/dev/null
printf {} "$log_guid"
printf '%s\n' '{{"guid":"00000000-0000-0000-0000-000000000000","status":"closed","contents":null}}'
"#,
        reply("ready", json!(null)),
        shlex::try_quote(request_path.to_str().unwrap()).unwrap(),
        reply("data", json!({"event":"loading","indices":[0,1]})),
        reply(
            "data",
            json!({"event":"item","indices":[0,1],"outcome":{"status":"summary","summary":{"size":3,"sha1":sha1,"murmur2":123456}}})
        ),
        reply("data", json!({"event":"done","failed":0})),
        reply("finished", json!(null)),
        reply("finished", json!(null)),
    );
    let mut session =
        Session::start(std::process::Command::new("sh").args(["-c", &script])).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut results = Vec::new();
    runtime.block_on(async {
        let sources = sources_for(address)
            .into_iter()
            .map(|source| {
                source.map_err(|message| ErrorReport {
                    message,
                    causes: Vec::new(),
                })
            })
            .collect();
        Identification::with_sources(
            vec![PathBuf::from(r"D:\remote-only.jar"); 2],
            sources,
            2,
            Location::Remote(&session, &RuntimeEnvironment::new().with_mods_cache(None)),
        )
        .unwrap()
        .run(|event| {
            results.push(event);
            Ok(())
        })
        .await
        .unwrap();
    });
    session.close().unwrap();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    let items: Vec<_> = results
        .iter()
        .filter_map(|event| {
            if let Event::Item {
                indices,
                source,
                outcome,
            } = event
            {
                Some((indices, source, outcome))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(items.len(), 2);
    for (indices, source, outcome) in items {
        assert_eq!(*indices, [0, 1]);
        let Outcome::Matched { file } = outcome else {
            panic!("expected verified local provider match: {outcome:?}")
        };
        assert_eq!(file.size, 3);
        if *source == 0 {
            assert_eq!(file.role, mcpm_backend::source::MatchedFileRole::Additional);
        } else {
            assert_eq!(file.reference.id.0, "456");
        }
    }
    let request: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(request_path).unwrap()).unwrap();
    assert_eq!(
        request["contents"],
        json!({"paths":[r"D:\remote-only.jar",r"D:\remote-only.jar"],"requirements":{"murmur2":true},"parallelism":2})
    );
}

#[test]
fn cache_events_preserve_hit_provenance_and_group_duplicate_physical_files() {
    use mcpm_backend::{
        local::cache::{CacheLinkPreference, mods::ModsCacheDir},
        source::{FileHash, FileId, FileMatch, FileRef, MatchedFileRole, ProjectId, ProjectRef},
    };
    use serde_json::json;

    let root = tempfile::tempdir().unwrap();
    let cache = ModsCacheDir::initialize(root.path().join("cache")).unwrap();
    let paths: Vec<_> = (0..3)
        .map(|index| {
            let path = root.path().join(format!("{index}.jar"));
            std::fs::write(&path, format!("archive {index}")).unwrap();
            path
        })
        .collect();
    let summaries: Vec<_> = paths
        .iter()
        .map(|path| FileSummary::read(path, SummaryRequirements { murmur2: true }).unwrap())
        .collect();
    for index in 0..2 {
        let id = cache
            .import(&paths[index], CacheLinkPreference::Copy)
            .unwrap();
        for source in if index == 0 {
            vec!["modrinth", "curseforge"]
        } else {
            vec!["modrinth"]
        } {
            cache
                .write_ident_info(
                    id,
                    &FileMatch {
                        reference: FileRef {
                            project: ProjectRef {
                                source: SourceId(source.into()),
                                id: ProjectId("7".into()),
                            },
                            id: FileId("10".into()),
                        },
                        filename: "cached.jar".into(),
                        size: summaries[index].size,
                        hashes: vec![FileHash::Sha1(summaries[index].sha1.clone())],
                        role: MatchedFileRole::Primary,
                    },
                )
                .unwrap();
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let modrinth = json!({
        "id": "version", "project_id": "project", "name": "Version", "version_number": "1",
        "version_type": "release", "date_published": "2026-01-01T00:00:00Z",
        "game_versions": [], "loaders": [], "dependencies": [],
        "files": [{"filename":"network.jar", "size":summaries[2].size, "primary":true,
            "url":"https://example.com/network.jar", "hashes":{"sha1":summaries[2].sha1}}]
    })
    .to_string();
    let curseforge =
        json!({"data":{"exactMatches": summaries[1..].iter().enumerate().map(|(index, summary)| {
        json!({"id":7,"file":{"id":index + 11,"modId":7,"fileName":"network.jar",
            "fileLength":summary.size,"fileFingerprint":summary.murmur2.unwrap(),
            "hashes":[{"algo":1,"value":summary.sha1}]}})
    }).collect::<Vec<_>>()}})
        .to_string();
    let server = thread::spawn(move || {
        for _ in 0..3 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "provider request timed out");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let request = read_request(&mut stream);
            let body = if request.starts_with("GET ") {
                &modrinth
            } else {
                &curseforge
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut events = Vec::new();
    runtime.block_on(async {
        let sources = sources_for(address)
            .into_iter()
            .map(|source| {
                source.map_err(|message| ErrorReport {
                    message,
                    causes: Vec::new(),
                })
            })
            .collect();
        let mut inputs = paths;
        inputs.push(inputs[0].clone());
        Identification::with_sources(
            inputs,
            sources,
            2,
            Location::Local(&RuntimeEnvironment::new().with_mods_cache(Some(
                ModsCacheSelection::Directory {
                    path: cache.path().to_owned(),
                },
            ))),
        )
        .unwrap()
        .run(|event| {
            events.push(event);
            Ok(())
        })
        .await
        .unwrap();
    });
    server.join().unwrap();
    let cache_events: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Cache { indices, sources } => Some((indices.clone(), sources.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(cache_events.len(), 3);
    for (indices, sources) in cache_events {
        assert_eq!(
            sources,
            match indices[0] {
                0 => vec![0, 1],
                1 => vec![0],
                _ => vec![],
            }
        );
        if indices[0] == 0 {
            assert_eq!(indices, [0, 3]);
        }
    }
    for (position, event) in events.iter().enumerate() {
        if let Event::Item {
            indices, outcome, ..
        } = event
        {
            assert!(matches!(outcome, Outcome::Matched { .. }));
            assert!(events[..position].iter().any(
                |event| matches!(event, Event::Cache { indices: checked, .. } if checked == indices)
            ));
        }
    }
}
