//! Local HTTP regression tests for lookup ordering, concurrency, deduplication and failures.

use super::*;
use kako_mods_lib::source::{SourceId, curseforge::CurseForgeSource, modrinth::ModrinthSource};
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
    assert_eq!(updates.len(), 8);
    for (index, pair) in updates.chunks_exact(2).enumerate() {
        assert!(matches!(pair[0].state, State::Loading));
        assert_eq!(pair[0].platform, Platform::ALL[index % 2]);
        assert_eq!(pair[1].platform, pair[0].platform);
        assert_eq!(pair[0].files, if index < 2 { vec![0, 2] } else { vec![1] });
        assert_eq!(pair[1].files, pair[0].files);
        if index == 0 {
            assert!(matches!(pair[1].state, State::Failed(_)));
        } else {
            assert!(matches!(pair[1].state, State::Unmatched));
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
    assert_eq!(updates.len(), 16);
    assert_eq!(
        updates
            .iter()
            .filter(|update| matches!(update.state, State::Unmatched))
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
