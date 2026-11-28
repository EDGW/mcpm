//! Frontend history replay: retain arrival-ordered logical lines across stdout and stderr
//! up to an inspect snapshot, then follow the same unified log Channel.

use anyhow::Result;
use mcpm_backend::operations::server::shim::{
    CursorRange, LogBatch, LogChunk, LogsInput, MAX_LOG_BLOCK_BYTES, ShimRef, Source, SubscribeLogs,
};
use mcpm_exec::{operation::StreamingOutput, service::Executor};
use std::collections::VecDeque;

pub(super) struct Opened {
    pub channel: StreamingOutput<LogBatch>,
    pub pending: Option<LogBatch>,
    pub history: VecDeque<(Source, Vec<u8>)>,
}

pub(super) async fn open(
    executor: &Executor,
    reference: &ShimRef,
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
            cursor: start,
            tail: None,
            max_bytes: MAX_LOG_BLOCK_BYTES,
            max_items: 32,
        })
        .await?;
    let mut tail = Tail::new(lines.unwrap_or(0));
    let mut position = start.unwrap_or(0);
    let mut pending: Option<LogBatch> = None;
    if lines.is_some_and(|lines| lines > 0) {
        while position < range.latest {
            let Some(batch) = channel.next().await else {
                break;
            };
            let batch = batch?;
            if batch.gap.is_some() {
                // Never glue pre-gap partial bytes onto post-gap chunks.
                tail.finish();
            }
            let mut live = Vec::new();
            for chunk in batch.batch {
                let end = range
                    .latest
                    .saturating_sub(chunk.seq)
                    .min(chunk.bytes.len() as u64) as usize;
                if end > 0 {
                    tail.push(chunk.source, &chunk.bytes[..end]);
                }
                if end < chunk.bytes.len() {
                    live.push(LogChunk {
                        seq: chunk.seq + end as u64,
                        source: chunk.source,
                        bytes: chunk.bytes[end..].to_vec(),
                    });
                }
            }
            position = batch.next_cursor;
            let live_gap = batch.gap.filter(|gap| gap.to > range.latest);
            pending = Some(LogBatch {
                batch: live,
                next_cursor: position,
                gap: pending
                    .as_ref()
                    .and_then(|existing| existing.gap.clone())
                    .or(live_gap),
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
    lines: VecDeque<(Source, Vec<u8>)>,
    partial: [Vec<u8>; 2],
    last_partial: Option<Source>,
    limit: usize,
}

impl Tail {
    fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            partial: [Vec::new(), Vec::new()],
            last_partial: None,
            limit,
        }
    }

    fn push(&mut self, source: Source, bytes: &[u8]) {
        let idx = usize::from(source == Source::Stderr);
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            self.partial[idx].extend_from_slice(part);
            if part.ends_with(b"\n") {
                self.finish_source(source);
            } else {
                self.last_partial = Some(source);
            }
        }
    }

    fn finish_source(&mut self, source: Source) {
        let idx = usize::from(source == Source::Stderr);
        if self.partial[idx].is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.partial[idx]);
        self.lines.push_back((source, line));
        while self.lines.len() > self.limit {
            self.lines.pop_front();
        }
        if self.last_partial == Some(source) {
            self.last_partial = None;
        }
    }

    fn finish(&mut self) {
        match self.last_partial {
            Some(Source::Stdout) => {
                self.finish_source(Source::Stderr);
                self.finish_source(Source::Stdout);
            }
            _ => {
                self.finish_source(Source::Stdout);
                self.finish_source(Source::Stderr);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_preserve_interleaved_sources_and_unterminated_raw_bytes() {
        let mut tail = Tail::new(3);
        tail.push(Source::Stdout, b"old\nfir");
        tail.push(Source::Stderr, b"warn\n");
        tail.push(Source::Stdout, b"st\r\n\xffpar");
        tail.push(Source::Stdout, b"tial");
        tail.finish();
        assert_eq!(
            tail.lines.into_iter().collect::<Vec<_>>(),
            vec![
                (Source::Stderr, b"warn\n".to_vec()),
                (Source::Stdout, b"first\r\n".to_vec()),
                (Source::Stdout, b"\xffpartial".to_vec()),
            ]
        );
    }
}
