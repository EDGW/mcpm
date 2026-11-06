//! Byte-bounded source buffers with loss-explicit cursors and independently readable tails.

use crate::operations::server::shim::{CursorRange, Gap, LogBatch, LogChunk, LogsInput};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const CAPACITY: usize = 1024 * 1024;
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Buffer {
    chunks: VecDeque<LogChunk>,
    latest: u64,
    size: usize,
    pub closed: bool,
}
impl Buffer {
    pub fn push(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let seq = self.latest;
        self.latest += bytes.len() as u64;
        self.size += bytes.len();
        self.chunks.push_back(LogChunk { seq, bytes });
        while self.size > CAPACITY {
            if let Some(chunk) = self.chunks.pop_front() {
                self.size -= chunk.bytes.len();
            }
        }
    }
    pub fn range(&self) -> CursorRange {
        CursorRange {
            oldest: self.chunks.front().map_or(self.latest, |chunk| chunk.seq),
            latest: self.latest,
        }
    }
    pub fn read(&self, input: &LogsInput) -> Result<LogBatch> {
        let range = self.range();
        let requested = input.cursor.unwrap_or_else(|| match input.tail {
            Some(tail) => range.latest.saturating_sub(tail as u64).max(range.oldest),
            None => range.oldest,
        });
        ensure!(
            requested <= range.latest,
            "log cursor is beyond this run's output"
        );
        let gap = (requested < range.oldest).then_some(Gap {
            from: requested,
            to: range.oldest,
        });
        let mut next_cursor = requested.max(range.oldest);
        let mut remaining = input.max_bytes.clamp(1, 256 * 1024);
        let mut batch = Vec::new();
        for chunk in &self.chunks {
            let end = chunk.seq + chunk.bytes.len() as u64;
            if end <= next_cursor {
                continue;
            }
            if batch.len() >= input.max_items.clamp(1, 128) || remaining == 0 {
                break;
            }
            let begin = (next_cursor - chunk.seq) as usize;
            let length = (chunk.bytes.len() - begin).min(remaining);
            batch.push(LogChunk {
                seq: next_cursor,
                bytes: chunk.bytes[begin..begin + length].to_vec(),
            });
            next_cursor += length as u64;
            remaining -= length;
        }
        Ok(LogBatch {
            batch,
            next_cursor,
            gap,
            finished: self.closed && next_cursor == range.latest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::server::shim::{ShimRef, Source};
    fn input(cursor: Option<u64>, tail: Option<usize>, max_bytes: usize) -> LogsInput {
        LogsInput {
            reference: ShimRef {
                server: ".".into(),
                run_id: "test".into(),
            },
            source: Source::Stdout,
            cursor,
            tail,
            max_bytes,
            max_items: 128,
        }
    }
    #[test]
    fn tail_split_resume_and_overflow_are_loss_explicit() -> Result<()> {
        let mut buffer = Buffer::default();
        buffer.push(b"abcdef".to_vec());
        let first = buffer.read(&input(None, Some(3), 2))?;
        assert_eq!(first.batch[0].bytes, b"de");
        assert_eq!(first.next_cursor, 5);
        let second = buffer.read(&input(Some(first.next_cursor), None, 2))?;
        assert_eq!(second.batch[0].bytes, b"f");
        buffer.push(vec![b'x'; CAPACITY]);
        let lost = buffer.read(&input(Some(5), None, 10))?;
        assert_eq!((lost.gap.unwrap().from, lost.next_cursor), (5, 16));
        assert!(!lost.finished);
        buffer.closed = true;
        assert!(
            buffer
                .read(&input(Some(buffer.range().latest), None, 10))?
                .finished
        );
        Ok(())
    }
}
