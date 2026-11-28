//! Unified arrival-ordered log journal with per-source byte quotas, loss-explicit cursors,
//! framed binary persistence, and generation-scoped crash report tail scanning.

use crate::operations::server::shim::{
    CursorRange, Gap, LogBatch, LogChunk, LogsInput, MAX_LOG_BLOCK_BYTES, Source,
};
use anyhow::{Context, Result, bail, ensure};
use mcpm_lib::local::mc::log::{clean_console_line, parse_header};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Maximum retained bytes per source (`stdout` and `stderr` each have an independent 1 MiB quota).
const CAPACITY_PER_SOURCE: usize = MAX_LOG_BLOCK_BYTES;
/// Coalesce tiny consecutive same-source reads up to 8 KiB without changing cross-source ordering.
const MERGE_CHUNK_BYTES: usize = 8 * 1024;
const FRAMED_LOG_MAGIC: &[u8; 8] = b"MCPMLOG\x01";
const CRASH_REPORT_MARKER: &str = "This crash report has been saved to:";
const CRASH_REPORT_SCAN_LINES: usize = 200;

/// Scans at most the last 200 logical lines of stdout (then stderr) written at or after
/// `since_cursor` for Minecraft's `ERROR`/`FATAL`/`SEVERE` crash report notices, returning
/// distinct candidate paths (newest first) resolved against `working_directory`.
/// Authoritative filesystem containment and validity checks are enforced by the caller.
pub(super) fn detect_crash_report_candidates(
    logs: &Buffer,
    working_directory: &Path,
    since_cursor: u64,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    logs.collect_crash_report_candidates(
        Source::Stdout,
        working_directory,
        since_cursor,
        &mut candidates,
    );
    logs.collect_crash_report_candidates(
        Source::Stderr,
        working_directory,
        since_cursor,
        &mut candidates,
    );
    candidates
}

#[derive(Default)]
pub(super) struct Buffer {
    chunks: VecDeque<LogChunk>,
    latest: u64,
    source_bytes: [usize; 2],
    pub closed: bool,
}

impl Buffer {
    pub fn push(&mut self, source: Source, mut bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let mut seq = self.latest;
        self.latest += bytes.len() as u64;
        if bytes.len() > CAPACITY_PER_SOURCE {
            let skip = bytes.len() - CAPACITY_PER_SOURCE;
            bytes.drain(..skip);
            seq += skip as u64;
        }
        let added = bytes.len();
        let idx = source_index(source);
        self.source_bytes[idx] += added;

        if let Some(last) = self.chunks.back_mut()
            && last.source == source
            && last.seq + last.bytes.len() as u64 == seq
            && last.bytes.len() + added <= MERGE_CHUNK_BYTES
        {
            last.bytes.extend_from_slice(&bytes);
        } else {
            self.chunks.push_back(LogChunk { seq, source, bytes });
        }

        // Enforce an independent 1 MiB cap per source so heavy stdout never evicts stderr.
        while self.source_bytes[idx] > CAPACITY_PER_SOURCE {
            let Some(pos) = self.chunks.iter().position(|chunk| chunk.source == source) else {
                break;
            };
            if let Some(evicted) = self.chunks.remove(pos) {
                self.source_bytes[idx] = self.source_bytes[idx].saturating_sub(evicted.bytes.len());
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
        let mut gap = (requested < range.oldest).then_some(Gap {
            from: requested,
            to: range.oldest,
        });
        let mut next_cursor = requested.max(range.oldest);
        let mut remaining = input.max_bytes.clamp(1, MAX_LOG_BLOCK_BYTES);
        let max_items = input.max_items.clamp(1, 128);
        let mut batch = Vec::new();

        for chunk in &self.chunks {
            let end = chunk.seq + chunk.bytes.len() as u64;
            if end <= next_cursor {
                continue;
            }
            if next_cursor < chunk.seq {
                // An internal gap exists where chunks of one source were evicted while an
                // earlier chunk from the other source remained retained.
                if !batch.is_empty() {
                    break;
                }
                gap = Some(match gap {
                    Some(existing) => Gap {
                        from: existing.from,
                        to: chunk.seq,
                    },
                    None => Gap {
                        from: next_cursor,
                        to: chunk.seq,
                    },
                });
                next_cursor = chunk.seq;
            }
            if batch.len() >= max_items || remaining == 0 {
                break;
            }
            let begin = (next_cursor - chunk.seq) as usize;
            let length = (chunk.bytes.len() - begin).min(remaining);
            batch.push(LogChunk {
                seq: next_cursor,
                source: chunk.source,
                bytes: chunk.bytes[begin..begin + length].to_vec(),
            });
            next_cursor += length as u64;
            remaining -= length;
        }

        // If all bytes at or after `next_cursor` were evicted (only possible if `chunks` is empty).
        if batch.is_empty() && next_cursor < range.latest {
            gap = Some(match gap {
                Some(existing) => Gap {
                    from: existing.from,
                    to: range.latest,
                },
                None => Gap {
                    from: next_cursor,
                    to: range.latest,
                },
            });
            next_cursor = range.latest;
        }

        Ok(LogBatch {
            batch,
            next_cursor,
            gap,
            finished: self.closed && next_cursor == range.latest,
        })
    }

    /// Writes framed chunks (`[seq:u64][source:u8][len:u32][bytes]`) before publishing the receipt.
    pub fn persist(&self, directory: &Path, run_id: &str) -> Result<()> {
        let mut file = super::private_file(&log_path(directory, run_id)?)?;
        file.write_all(FRAMED_LOG_MAGIC)?;
        for chunk in &self.chunks {
            let len = u32::try_from(chunk.bytes.len()).context("log chunk exceeds u32 length")?;
            file.write_all(&chunk.seq.to_le_bytes())?;
            file.write_all(&[encode_source(chunk.source)])?;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(&chunk.bytes)?;
        }
        file.flush()?;
        file.sync_all().context("failed to sync retained log bytes")
    }

    pub fn restore(directory: &Path, run_id: &str, range: &CursorRange) -> Result<Self> {
        ensure!(
            range.oldest <= range.latest,
            "invalid retained log cursor range"
        );
        let path = log_path(directory, run_id)?;
        let mut file = std::fs::File::open(&path)
            .with_context(|| format!("failed to open retained log {path:?}"))?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)
            .context("retained log is missing framed header")?;
        ensure!(
            &magic == FRAMED_LOG_MAGIC,
            "unsupported or corrupted retained log header"
        );

        let mut chunks = VecDeque::new();
        let mut source_bytes = [0usize; 2];
        let mut prev_end = range.oldest;
        let mut header = [0u8; 13];
        loop {
            match read_exact_or_eof(&mut file, &mut header)? {
                false => break,
                true => {}
            }
            let seq = u64::from_le_bytes(header[0..8].try_into().unwrap());
            let source = decode_source(header[8])?;
            let len = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;
            ensure!(len > 0 && len <= CAPACITY_PER_SOURCE, "invalid retained chunk length");
            ensure!(
                seq >= prev_end,
                "retained log chunks are out of order or overlapping"
            );
            let end = seq
                .checked_add(len as u64)
                .context("retained log sequence overflow")?;
            ensure!(
                end <= range.latest,
                "retained log chunk exceeds receipt latest cursor"
            );
            let idx = source_index(source);
            source_bytes[idx] = source_bytes[idx]
                .checked_add(len)
                .context("retained log byte count overflow")?;
            ensure!(
                source_bytes[idx] <= CAPACITY_PER_SOURCE,
                "retained log exceeds per-source byte limit"
            );
            let mut bytes = vec![0u8; len];
            file.read_exact(&mut bytes)
                .context("retained log ended mid-chunk")?;
            chunks.push_back(LogChunk { seq, source, bytes });
            prev_end = end;
        }

        if let Some(first) = chunks.front() {
            ensure!(
                first.seq == range.oldest,
                "retained log oldest cursor does not match receipt"
            );
            ensure!(
                prev_end == range.latest,
                "retained log ended before its declared latest cursor"
            );
        } else {
            ensure!(
                range.oldest == range.latest,
                "empty retained log does not match non-empty receipt range"
            );
        }

        Ok(Self {
            chunks,
            latest: range.latest,
            source_bytes,
            closed: true,
        })
    }

    fn collect_crash_report_candidates(
        &self,
        source: Source,
        working_directory: &Path,
        since_cursor: u64,
        candidates: &mut Vec<PathBuf>,
    ) {
        let tail = self.tail_lines_bytes(source, CRASH_REPORT_SCAN_LINES, since_cursor);
        if tail.is_empty() {
            return;
        }
        let working_directory = normalize_lexical(&strip_windows_verbatim(working_directory));
        let text = String::from_utf8_lossy(&tail);
        for line in text.lines().rev() {
            let clean = clean_console_line(line);
            let Some(header) = parse_header(&clean) else {
                continue;
            };
            if !header.level.is_error_or_fatal() {
                continue;
            }
            let Some(raw_path) = header.body.strip_prefix(CRASH_REPORT_MARKER) else {
                continue;
            };
            let trimmed = raw_path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = strip_windows_verbatim(Path::new(trimmed));
            let resolved = if candidate.is_absolute() {
                candidate
            } else {
                working_directory.join(candidate)
            };
            let normalized = normalize_lexical(&resolved);
            if !candidates.contains(&normalized) {
                candidates.push(normalized);
            }
        }
    }

    /// Collects bytes covering at most the last `max_lines` logical lines of `source` across
    /// chunk boundaries, restricted to bytes at sequence offset `>= since_cursor`.
    fn tail_lines_bytes(&self, source: Source, max_lines: usize, since_cursor: u64) -> Vec<u8> {
        if max_lines == 0 || self.chunks.is_empty() || self.latest <= since_cursor {
            return Vec::new();
        }
        let mut reversed = Vec::new();
        let mut newlines = 0usize;
        let mut seen_any = false;
        'outer: for chunk in self.chunks.iter().rev() {
            let chunk_end = chunk.seq + chunk.bytes.len() as u64;
            if chunk_end <= since_cursor {
                break;
            }
            if chunk.source != source {
                continue;
            }
            let slice_start = since_cursor.saturating_sub(chunk.seq) as usize;
            for &byte in chunk.bytes[slice_start..].iter().rev() {
                if byte == b'\n' && seen_any {
                    newlines += 1;
                    if newlines >= max_lines {
                        break 'outer;
                    }
                }
                // A single trailing `\n` sets `seen_any` without incrementing `newlines`,
                // keeping the terminator byte so `str::lines()` strips CRLF cleanly.
                seen_any = true;
                reversed.push(byte);
            }
        }
        reversed.reverse();
        reversed
    }
}

fn source_index(source: Source) -> usize {
    usize::from(source == Source::Stderr)
}

fn encode_source(source: Source) -> u8 {
    match source {
        Source::Stdout => 0,
        Source::Stderr => 1,
    }
}

fn decode_source(byte: u8) -> Result<Source> {
    match byte {
        0 => Ok(Source::Stdout),
        1 => Ok(Source::Stderr),
        other => bail!("invalid retained log source tag {other}"),
    }
}

fn read_exact_or_eof(reader: &mut impl Read, buf: &mut [u8]) -> Result<bool> {
    let mut offset = 0;
    while offset < buf.len() {
        match reader.read(&mut buf[offset..]) {
            Ok(0) => {
                if offset == 0 {
                    return Ok(false);
                }
                bail!("retained log ended inside a frame header");
            }
            Ok(n) => offset += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("failed to read retained log frame header"),
        }
    }
    Ok(true)
}

/// Strips Windows verbatim prefixes (`\\?\` / `\\?\UNC\`) so `Path::components()` normalizes
/// `.` / `..` segments and display paths stay readable.
pub(super) fn strip_windows_verbatim(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn log_path(directory: &Path, run_id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(run_id).context("invalid retained log run id")?;
    Ok(directory.join(format!("logs-{run_id}.bin")))
}

/// Keep the latest published receipt's log file while removing older generated run files
/// (including legacy split `logs-<id>-stdout.bin` / `logs-<id>-stderr.bin` files).
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
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        let is_legacy = matches!(suffix, "-stdout.bin" | "-stderr.bin");
        let is_current = suffix == ".bin" && Some(id) != retained;
        if is_legacy || is_current {
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
            cursor,
            tail,
            max_bytes,
            max_items: 128,
        }
    }

    #[test]
    fn tail_split_resume_and_per_source_quota_eviction_are_loss_explicit() -> Result<()> {
        let mut buffer = Buffer::default();
        buffer.push(Source::Stderr, b"ERR!".to_vec());
        buffer.push(Source::Stdout, b"abcdef".to_vec());

        // Reading across interleaved sources preserves exact arrival order.
        let first = buffer.read(&input(Some(0), None, 6))?;
        assert_eq!(first.batch.len(), 2);
        assert_eq!((first.batch[0].source, first.batch[0].bytes.as_slice()), (Source::Stderr, b"ERR!".as_slice()));
        assert_eq!((first.batch[1].source, first.batch[1].bytes.as_slice()), (Source::Stdout, b"ab".as_slice()));
        assert_eq!(first.next_cursor, 6);

        // Overflowing stdout evicts old stdout ("abcdef") while retaining earlier stderr ("ERR!").
        buffer.push(Source::Stdout, vec![b'x'; CAPACITY_PER_SOURCE]);
        let before_gap = buffer.read(&input(Some(0), None, 10))?;
        assert_eq!(before_gap.batch.len(), 1);
        assert_eq!(before_gap.batch[0].source, Source::Stderr);
        assert_eq!(before_gap.batch[0].bytes, b"ERR!");
        assert_eq!(before_gap.next_cursor, 4);
        assert!(before_gap.gap.is_none());

        // The next read reports the internal gap 4..10 where stdout was evicted.
        let across_gap = buffer.read(&input(Some(4), None, 10))?;
        assert_eq!(across_gap.gap.as_ref().map(|g| (g.from, g.to)), Some((4, 10)));
        assert_eq!(across_gap.batch[0].source, Source::Stdout);
        assert_eq!(across_gap.batch[0].seq, 10);
        assert_eq!(across_gap.next_cursor, 20);
        assert!(!across_gap.finished);

        buffer.closed = true;
        assert!(
            buffer
                .read(&input(Some(buffer.range().latest), None, 10))?
                .finished
        );
        Ok(())
    }

    #[test]
    fn framed_persist_preserves_interleaved_sources_gaps_and_rejects_corrupt_files() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let run = "00000000-0000-0000-0000-000000000001";
        let mut buffer = Buffer::default();
        buffer.push(Source::Stderr, b"early-err\n".to_vec());
        buffer.push(Source::Stdout, b"evicted\n".to_vec());
        let flood: Vec<u8> = (0..CAPACITY_PER_SOURCE)
            .map(|index| (index % 256) as u8)
            .collect();
        buffer.push(Source::Stdout, flood.clone());
        buffer.push(Source::Stderr, b"late-err\n".to_vec());
        buffer.closed = true;
        let range = buffer.range();
        buffer.persist(directory.path(), run)?;

        let loaded = Buffer::restore(directory.path(), run, &range)?;
        let first = loaded.read(&input(Some(0), None, MAX_LOG_BLOCK_BYTES))?;
        assert_eq!(first.batch.len(), 1);
        assert_eq!(first.batch[0].source, Source::Stderr);
        assert_eq!(first.batch[0].bytes, b"early-err\n");
        assert!(first.gap.is_none());

        let second = loaded.read(&input(Some(first.next_cursor), None, MAX_LOG_BLOCK_BYTES))?;
        assert_eq!(second.gap.as_ref().map(|g| (g.from, g.to)), Some((10, 18)));
        assert_eq!(second.batch[0].source, Source::Stdout);
        assert_eq!(second.batch[0].bytes, flood);

        let third = loaded.read(&input(Some(second.next_cursor), None, MAX_LOG_BLOCK_BYTES))?;
        assert_eq!(third.batch[0].source, Source::Stderr);
        assert_eq!(third.batch[0].bytes, b"late-err\n");
        assert!(third.finished);

        std::fs::write(log_path(directory.path(), run)?, b"truncated")?;
        assert!(Buffer::restore(directory.path(), run, &range).is_err());
        Ok(())
    }

    #[test]
    fn detects_crash_report_candidates_across_loaders_and_rejects_forgery() {
        let wd = Path::new("/srv/minecraft");

        // 1.6.4 logs to stderr with an absolute path containing `/./`.
        let mut logs = Buffer::default();
        logs.push(
            Source::Stderr,
            b"2013-10-15 16:20:00 [SEVERE] This crash report has been saved to: /srv/minecraft/./crash-reports/crash-2013-10-15_16.20.00-server.txt\r\n"
                .to_vec(),
        );
        assert_eq!(
            detect_crash_report_candidates(&logs, wd, 0),
            vec![PathBuf::from(
                "/srv/minecraft/crash-reports/crash-2013-10-15_16.20.00-server.txt"
            )]
        );

        // Modern 1.21+ relative path with JLine prompt overwrite + ANSI reset codes split across
        // chunks, followed by spoofed player chat, prefixed error lines, and bare lines.
        let mut logs = Buffer::default();
        logs.push(
            Source::Stdout,
            b"\x1b[m> \r  \r\x1b[31m[12:00:00] [Server thread/ERROR] [minecraft/MinecraftServer]: This crash report has been ".to_vec(),
        );
        logs.push(
            Source::Stdout,
            b"saved to: crash-reports/crash-2026-05-01_12.00.00-server.txt\x1b[m\n".to_vec(),
        );
        logs.push(
            Source::Stdout,
            concat!(
                "[12:00:01] [Server thread/INFO]: <Steve> This crash report has been saved to: crash-reports/fake.txt\n",
                "[12:00:01 INFO]: [Steve]: This crash report has been saved to: crash-reports/fake2.txt\n",
                "[12:00:01] [Server thread/ERROR]: Failed for Steve: This crash report has been saved to: crash-reports/fake3.txt\n",
                "This crash report has been saved to: crash-reports/bare.txt\n",
            )
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(
            detect_crash_report_candidates(&logs, wd, 0),
            vec![PathBuf::from(
                "/srv/minecraft/crash-reports/crash-2026-05-01_12.00.00-server.txt"
            )]
        );
        // Generation boundary excludes crash reports from earlier generations.
        let next_gen = logs.range().latest;
        logs.push(
            Source::Stdout,
            b"[12:01:00] [Server thread/INFO]: Clean restart\n".to_vec(),
        );
        assert!(detect_crash_report_candidates(&logs, wd, next_gen).is_empty());

        // Paper & Forge headers plus 200-line tail window: 199 trailing lines still match, 200 evict it.
        let mut logs = Buffer::default();
        logs.push(
            Source::Stdout,
            b"[06Nov2026 12:00:00.123] [Server Watchdog/FATAL] [net.minecraft.server.dedicated.ServerWatchdog/]: This crash report has been saved to: crash-reports/old.txt\n"
                .to_vec(),
        );
        for _ in 0..199 {
            logs.push(Source::Stdout, b"noise line\n".to_vec());
        }
        assert_eq!(
            detect_crash_report_candidates(&logs, wd, 0),
            vec![PathBuf::from("/srv/minecraft/crash-reports/old.txt")]
        );
        logs.push(Source::Stdout, b"one more noise line\n".to_vec());
        assert!(detect_crash_report_candidates(&logs, wd, 0).is_empty());
    }
}
