//! HTTP regressions for attempt-local progress, unknown lengths and observer failure cleanup.

use std::{
    cell::RefCell,
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use super::*;
use sources::{MappedSource, UrlMapping};

#[test]
fn fallback_resets_bytes_and_completion_requires_a_verified_published_file() {
    for (second_body, second_length) in [
        (b"agent".as_slice(), false),
        (b"agent".as_slice(), true),
        (b"broken".as_slice(), false),
    ] {
        let fixture = Fixture::new(vec![(b"broken", true), (second_body, second_length)]);
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("agent.jar");
        // Quiet APIs keep their Send futures even though observers may opt into local state.
        fn assert_send(_: impl Send) {}
        assert_send(fixture.downloader.download_to(&request(), &target));
        assert_send(authlib_injector::latest(&fixture.downloader));
        let events = RefCell::new(Vec::new());
        let result = runtime().block_on(fixture.downloader.download_to_with_progress(
            &request(),
            &target,
            &|event| {
                // Completion is emitted only after publication, not on the final byte.
                if matches!(event, Event::Completed { .. }) {
                    assert_eq!(std::fs::read(&target).unwrap(), b"agent");
                }
                events.borrow_mut().push(Recorded::from(event));
                Ok(())
            },
        ));
        let events = events.into_inner();
        let second = events
            .iter()
            .position(|event| *event == Recorded::Started("second".into()))
            .unwrap();
        assert_eq!(events[0], Recorded::Started("first".into()));
        assert_eq!(events[1], Recorded::Bytes(0, Some(6)));
        assert!(events[..second].contains(&Recorded::Bytes(6, Some(6))));
        assert!(events[..second].contains(&Recorded::Verifying));
        assert!(events[..second].contains(&Recorded::Failed));
        let total = second_length.then_some(second_body.len() as u64);
        assert_eq!(events[second + 1], Recorded::Bytes(0, total));
        assert!(events[second..].contains(&Recorded::Bytes(second_body.len() as u64, total)));
        if second_body == b"agent" {
            result.unwrap();
            assert_eq!(events.last(), Some(&Recorded::Completed));
        } else {
            assert!(result.unwrap_err().is::<crate::error::CombinedError>());
            assert!(!events.contains(&Recorded::Completed));
            assert!(!target.exists());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
        fixture.thread.join().unwrap();
    }
}

#[test]
fn observer_failure_during_transfer_aborts_without_fallback_or_publication() {
    #[derive(Debug, thiserror::Error)]
    #[error("progress consumer failed")]
    struct ConsumerFailed;

    for fail_on_notification in [false, true] {
        let body = if fail_on_notification {
            b"broken".as_slice()
        } else {
            b"agent".as_slice()
        };
        let fixture = Fixture::new(vec![(body, true)]);
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("agent.jar");
        let events = RefCell::new(Vec::new());
        let error = runtime()
            .block_on(
                fixture
                    .downloader
                    .download_to_with_progress(&request(), &target, &|event| {
                        events.borrow_mut().push(Recorded::from(event));
                        if (fail_on_notification && matches!(event, Event::AttemptFailed { .. }))
                            || (!fail_on_notification
                                && matches!(event, Event::Bytes { received, .. } if received > 0))
                        {
                            return Err(ConsumerFailed.into());
                        }
                        Ok(())
                    }),
            )
            .unwrap_err();
        assert!(crate::error::contains::<ConsumerFailed>(error.as_ref()));
        if fail_on_notification {
            assert!(crate::error::contains::<ChecksumMismatch>(error.as_ref()));
        }
        assert!(
            !events
                .borrow()
                .contains(&Recorded::Started("second".into()))
        );
        assert!(!events.borrow().contains(&Recorded::Completed));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        fixture.thread.join().unwrap();
    }
}

fn request() -> DownloadRequest {
    DownloadRequest::new(
        "https://example.invalid/file".parse().unwrap(),
        None,
        vec![
            Checksum::new(
                ChecksumAlgorithm::Sha256,
                "d4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824",
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[derive(Debug, PartialEq)]
enum Recorded {
    Started(String),
    Bytes(u64, Option<u64>),
    Verifying,
    Failed,
    Completed,
}

impl From<Event<'_>> for Recorded {
    fn from(event: Event<'_>) -> Self {
        match event {
            Event::AttemptStarted { source, .. } => Self::Started(source.into()),
            Event::Bytes { received, total } => Self::Bytes(received, total),
            Event::Verifying => Self::Verifying,
            Event::AttemptFailed { .. } => Self::Failed,
            Event::Completed { .. } => Self::Completed,
        }
    }
}

/// Ordered responses let tests prove that fallback retains the same request and resets progress.
struct Fixture {
    downloader: Downloader,
    thread: thread::JoinHandle<()>,
}

impl Fixture {
    fn new(responses: Vec<(&[u8], bool)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sources: Vec<Arc<dyn DownloadSource>> = ["first", "second"]
            .into_iter()
            .map(|name| {
                Arc::new(
                    MappedSource::new(
                        name,
                        vec![
                            UrlMapping::new(
                                "https://example.invalid/".parse().unwrap(),
                                format!("http://{address}/{name}/").parse().unwrap(),
                            )
                            .unwrap(),
                        ],
                    )
                    .unwrap(),
                ) as Arc<dyn DownloadSource>
            })
            .collect();
        let downloader = Downloader::new(sources, DownloadOptions::default()).unwrap();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(body, length)| (body.to_vec(), length))
            .collect();
        let thread = thread::spawn(move || {
            for (index, (body, length)) in responses.into_iter().enumerate() {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "download request timed out");
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("fixture accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let source = if index == 0 { "first" } else { "second" };
                assert!(
                    String::from_utf8(request)
                        .unwrap()
                        .starts_with(&format!("GET /{source}/file HTTP/1.1\r\n"))
                );
                write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n").unwrap();
                if length {
                    write!(stream, "Content-Length: {}\r\n", body.len()).unwrap();
                }
                stream.write_all(b"\r\n").unwrap();
                let _ = stream.write_all(&body);
            }
        });
        Self { downloader, thread }
    }
}
