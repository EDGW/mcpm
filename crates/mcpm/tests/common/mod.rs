//! Shared driver for integration tests that speak protocol v6 to a serve process directly.
//!
//! Serve cancels accepted calls when its stdin closes, so a caller that writes requests must
//! keep stdin open until the replies arrive. This driver does that and returns the complete
//! frame stream, including the connection's closing frame.

#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use uuid::Uuid;

/// One serve process driven request by request.
pub struct Remote {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    replies: Vec<Value>,
    pending: usize,
}

/// Everything the serve process produced before it exited.
pub struct Finished {
    pub frames: Vec<Value>,
    pub stderr: Vec<u8>,
}

impl Remote {
    /// Drives a serve process the caller already spawned with piped standard streams.
    pub fn from_child(mut child: Child) -> Self {
        let input = child.stdin.take().expect("piped stdin");
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        Self {
            child,
            input: Some(input),
            output,
            replies: Vec::new(),
            pending: 0,
        }
    }

    pub fn start(command: &mut Command) -> Self {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("serve process must start");
        let input = child.stdin.take().expect("piped stdin");
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        Self {
            child,
            input: Some(input),
            output,
            replies: Vec::new(),
            pending: 0,
        }
    }

    /// Writes one cancellation frame; the call still ends with its terminal frame.
    pub fn send_cancel(&mut self, guid: Uuid) {
        self.write(&json!({"cancel": guid}));
    }

    /// Writes one request; stdin stays open so its replies can be read.
    pub fn send(&mut self, request: Value) {
        self.pending += 1;
        self.write(&request);
    }

    fn write(&mut self, message: &Value) {
        let input = self.input.as_mut().expect("stdin stays open until finish");
        writeln!(input, "{message}").expect("message must be written");
    }

    /// Reads until every sent call ended, closes stdin and returns the whole frame stream.
    pub fn finish(mut self) -> Finished {
        while self.pending > 0 {
            let reply = self.next();
            if matches!(reply["status"].as_str(), Some("finished" | "failed")) {
                self.pending -= 1;
            }
        }
        drop(self.input.take());
        while self.next_optional().is_some() {}
        let mut stderr = Vec::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            let _ = pipe.read_to_end(&mut stderr);
        }
        let status = self.child.wait().expect("serve process must be waited");
        assert!(
            status.success(),
            "serve process failed with {status}: {}",
            String::from_utf8_lossy(&stderr)
        );
        Finished {
            frames: self.replies,
            stderr,
        }
    }

    /// Reads one frame, recording it; panics if the process ended first.
    pub fn next_frame(&mut self) -> Value {
        self.next()
    }

    fn next(&mut self) -> Value {
        self.next_optional()
            .expect("serve process ended before the call did")
    }

    fn next_optional(&mut self) -> Option<Value> {
        let mut line = String::new();
        if self.output.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let frame: Value = serde_json::from_str(&line).expect("serve frames are JSON");
        self.replies.push(frame.clone());
        Some(frame)
    }
}
