//! Frontend history replay: retain logical lines up to an inspect snapshot, then follow the same Channel.

use anyhow::Result;
use mcpm_backend::operations::server::shim::{
    CursorRange, LogBatch, LogChunk, LogsInput, MAX_LOG_BLOCK_BYTES, ShimRef, Source, SubscribeLogs,
};
use mcpm_exec::{operation::StreamingOutput, service::Executor};
use std::collections::VecDeque;

pub(super) struct Opened {
    pub channel: StreamingOutput<LogBatch>,
    pub pending: Option<LogBatch>,
    pub history: VecDeque<Vec<u8>>,
}
pub(super) async fn open(
    executor: &Executor,
    reference: &ShimRef,
    source: Source,
    cursor: Option<u64>,
    range: &CursorRange,
    lines: Option<usize>,
) -> Result<Opened> {
    let start = match lines {
        Some(0) => Some(range.latest),
        Some(_) => Some(range.oldest),
        None => cursor,
    };
    let mut channel = executor
        .call::<SubscribeLogs>(LogsInput {
            reference: reference.clone(),
            source,
            cursor: start,
            tail: None,
            max_bytes: MAX_LOG_BLOCK_BYTES,
            max_items: 32,
        })
        .await?;
    let mut tail = Tail {
        lines: VecDeque::new(),
        partial: Vec::new(),
        limit: lines.unwrap_or(0),
    };
    let mut position = start.unwrap_or(0);
    let mut pending: Option<LogBatch> = None;
    if lines.is_some_and(|lines| lines > 0) {
        while position < range.latest {
            let Some(batch) = channel.next().await else {
                break;
            };
            let batch = batch?;
            let mut live = Vec::new();
            for chunk in batch.batch {
                let end = range
                    .latest
                    .saturating_sub(chunk.seq)
                    .min(chunk.bytes.len() as u64) as usize;
                if end > 0 {
                    tail.push(&chunk.bytes[..end]);
                }
                if end < chunk.bytes.len() {
                    live.push(LogChunk {
                        seq: chunk.seq + end as u64,
                        bytes: chunk.bytes[end..].to_vec(),
                    });
                }
            }
            position = batch.next_cursor;
            let gap = pending
                .as_ref()
                .and_then(|batch| batch.gap.clone())
                .or(batch.gap);
            pending = Some(LogBatch {
                batch: live,
                next_cursor: position,
                gap,
                finished: batch.finished,
            });
            if position >= range.latest || batch.finished {
                break;
            }
        }
    }
    tail.finish();
    Ok(Opened {
        channel,
        pending,
        history: tail.lines,
    })
}

struct Tail {
    lines: VecDeque<Vec<u8>>,
    partial: Vec<u8>,
    limit: usize,
}
impl Tail {
    fn push(&mut self, bytes: &[u8]) {
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            self.partial.extend_from_slice(part);
            if part.ends_with(b"\n") {
                self.finish();
            }
        }
    }
    fn finish(&mut self) {
        if self.partial.is_empty() {
            return;
        }
        self.lines.push_back(std::mem::take(&mut self.partial));
        while self.lines.len() > self.limit {
            self.lines.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragments_preserve_last_lines_and_unterminated_raw_bytes() {
        let mut tail = Tail {
            lines: VecDeque::new(),
            partial: Vec::new(),
            limit: 2,
        };
        for part in [b"old\nfir".as_slice(), b"st\r\n\xffpar", b"tial"] {
            tail.push(part);
        }
        tail.finish();
        assert_eq!(
            tail.lines.into_iter().collect::<Vec<_>>(),
            vec![b"first\r\n".to_vec(), b"\xffpartial".to_vec()]
        );
    }
}
