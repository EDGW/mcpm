//! Bounded console history used for attach replay.

use std::collections::VecDeque;

#[derive(Debug)]
pub struct OutputBuffer {
    next: u64,
    capacity: usize,
    lines: VecDeque<(u64, String, String)>,
}

impl OutputBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            next: 0,
            capacity: capacity.max(1),
            lines: VecDeque::new(),
        }
    }

    pub fn push(&mut self, stream: &str, text: String) -> (u64, Option<u64>) {
        let sequence = self.next;
        self.next += 1;
        self.lines.push_back((sequence, stream.to_owned(), text));
        let dropped = if self.lines.len() > self.capacity {
            self.lines.pop_front().map(|line| line.0)
        } else {
            None
        };
        (sequence, dropped)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn tail(&self, count: usize) -> impl Iterator<Item = &(u64, String, String)> {
        self.lines
            .iter()
            .skip(self.lines.len().saturating_sub(count))
    }
}
