//! Byte-bounded source buffers with loss-explicit cursors and independently readable tails.

use crate::operations::server::shim::{
    CursorRange, Gap, LogBatch, LogChunk, LogsInput, MAX_LOG_BLOCK_BYTES, Source,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const CAPACITY: usize = MAX_LOG_BLOCK_BYTES;
#[derive(Default)]
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
        let mut remaining = input.max_bytes.clamp(1, MAX_LOG_BLOCK_BYTES);
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

    /// Write raw source bytes before publishing the receipt that references their cursors.
    pub fn persist(&self, directory: &Path, run_id: &str, source: Source) -> Result<()> {
        let mut file = super::private_file(&log_path(directory, run_id, source)?)?;
        for chunk in &self.chunks {
            file.write_all(&chunk.bytes)?;
        }
        file.flush()?;
        file.sync_all().context("failed to sync retained log bytes")
    }

    pub fn restore(
        directory: &Path,
        run_id: &str,
        source: Source,
        range: &CursorRange,
    ) -> Result<Self> {
        ensure!(
            range.oldest <= range.latest,
            "invalid retained log cursor range"
        );
        let length = range.latest - range.oldest;
        ensure!(length <= CAPACITY as u64, "retained log exceeds byte limit");
        let path = log_path(directory, run_id, source)?;
        let file = std::fs::File::open(&path)
            .with_context(|| format!("failed to open retained log {path:?}"))?;
        ensure!(
            file.metadata()?.len() == length,
            "retained log size does not match receipt cursors"
        );
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(length).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == length,
            "retained log ended before its declared cursor"
        );
        let chunks = if bytes.is_empty() {
            VecDeque::new()
        } else {
            VecDeque::from([LogChunk {
                seq: range.oldest,
                bytes,
            }])
        };
        Ok(Self {
            chunks,
            latest: range.latest,
            size: length as usize,
            closed: true,
        })
    }
}

fn log_path(directory: &Path, run_id: &str, source: Source) -> Result<PathBuf> {
    uuid::Uuid::parse_str(run_id).context("invalid retained log run id")?;
    let source = match source {
        Source::Stdout => "stdout",
        Source::Stderr => "stderr",
    };
    Ok(directory.join(format!("logs-{run_id}-{source}.bin")))
}

/// Keep the latest published receipt's tails while removing older generated run files.
pub(super) fn remove_obsolete(directory: &Path, retained: Option<&str>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().and_then(|name| name.strip_prefix("logs-")) else {
            continue;
        };
        let Some((id, suffix)) = name.split_at_checked(36) else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_ok()
            && Some(id) != retained
            && matches!(suffix, "-stdout.bin" | "-stderr.bin")
        {
            std::fs::remove_file(entry.path()).context("failed to remove obsolete retained log")?;
        }
    }
    Ok(())
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

    #[test]
    fn binary_tail_preserves_bytes_cursors_and_rejects_inconsistent_files() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let run = "00000000-0000-0000-0000-000000000001";
        let mut buffer = Buffer::default();
        buffer.push(b"gone".to_vec());
        let bytes: Vec<u8> = (0..MAX_LOG_BLOCK_BYTES)
            .map(|index| (index % 256) as u8)
            .collect();
        buffer.push(bytes.clone());
        buffer.closed = true;
        let range = buffer.range();
        buffer.persist(directory.path(), run, Source::Stdout)?;
        let loaded = Buffer::restore(directory.path(), run, Source::Stdout, &range)?;
        let batch = loaded.read(&input(Some(0), None, MAX_LOG_BLOCK_BYTES * 2))?;
        assert_eq!(batch.gap.unwrap().to, 4);
        assert_eq!(batch.batch[0].seq, 4);
        assert_eq!(batch.batch[0].bytes, bytes);
        assert_eq!(batch.next_cursor, range.latest);
        assert!(batch.finished);
        std::fs::write(
            log_path(directory.path(), run, Source::Stdout)?,
            b"truncated",
        )?;
        assert!(Buffer::restore(directory.path(), run, Source::Stdout, &range).is_err());
        Ok(())
    }
}
