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
const CRASH_REPORT_MARKER: &str = "This crash report has been saved to:";
const CRASH_REPORT_SCAN_LINES: usize = 200;

/// Scans the last 200 logical lines of stdout and stderr for Minecraft's crash report notice,
/// resolving relative paths against Java's working directory.
pub(super) fn detect_crash_report(
    stdout: &Buffer,
    stderr: &Buffer,
    working_directory: &Path,
) -> Option<String> {
    stdout
        .detect_crash_report(working_directory)
        .or_else(|| stderr.detect_crash_report(working_directory))
}

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

    fn detect_crash_report(&self, working_directory: &Path) -> Option<String> {
        let tail = self.tail_lines_bytes(CRASH_REPORT_SCAN_LINES);
        if tail.is_empty() {
            return None;
        }
        let working_directory = strip_windows_verbatim(working_directory);
        let text = String::from_utf8_lossy(&tail);
        for line in text.lines().rev() {
            let clean = strip_ansi(line);
            let Some((_, raw_path)) = clean.rsplit_once(CRASH_REPORT_MARKER) else {
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
            return Some(normalize_lexical(&resolved).to_string_lossy().into_owned());
        }
        None
    }

    /// Collects bytes covering at most the last `max_lines` logical lines across chunk boundaries.
    fn tail_lines_bytes(&self, max_lines: usize) -> Vec<u8> {
        if max_lines == 0 || self.chunks.is_empty() {
            return Vec::new();
        }
        let mut reversed = Vec::new();
        let mut newlines = 0usize;
        let mut seen_any = false;
        'outer: for chunk in self.chunks.iter().rev() {
            for &byte in chunk.bytes.iter().rev() {
                if byte == b'\n' {
                    if seen_any {
                        newlines += 1;
                        if newlines >= max_lines {
                            break 'outer;
                        }
                    } else {
                        // A single trailing line terminator does not start an extra empty line.
                        seen_any = true;
                        continue;
                    }
                }
                seen_any = true;
                reversed.push(byte);
            }
        }
        reversed.reverse();
        reversed
    }
}

/// Removes ANSI CSI/OSC/ESC control sequences so color resets never stick to extracted paths.
fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\x07' {
                        break;
                    }
                    if next == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    output
}

/// Strips Windows verbatim prefixes (`\\?\` / `\\?\UNC\`) so `Path::components()` normalizes
/// `.` / `..` segments and display paths stay readable.
fn strip_windows_verbatim(path: &Path) -> PathBuf {
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

    #[test]
    fn detects_crash_report_paths_across_versions_chunks_ansi_and_200_line_limit() {
        let wd = Path::new("/srv/minecraft");

        // 1.6.4 logs to stderr with an absolute path containing `/./`.
        let stdout = Buffer::default();
        let mut stderr = Buffer::default();
        stderr.push(
            b"2013-10-15 16:20:00 [SEVERE] This crash report has been saved to: /srv/minecraft/./crash-reports/crash-2013-10-15_16.20.00-server.txt\r\n"
                .to_vec(),
        );
        assert_eq!(
            detect_crash_report(&stdout, &stderr, wd),
            Some("/srv/minecraft/crash-reports/crash-2013-10-15_16.20.00-server.txt".to_owned())
        );

        // Modern 1.21+ relative path with ANSI reset codes and split across chunks.
        let mut stdout = Buffer::default();
        let stderr = Buffer::default();
        stdout.push(b"\x1b[31m[12:00:00] [Server thread/ERROR]: This crash report has been ".to_vec());
        stdout.push(b"saved to: crash-reports/crash-2026-05-01_12.00.00-server.txt\x1b[m\n".to_vec());
        assert_eq!(
            detect_crash_report(&stdout, &stderr, wd),
            Some("/srv/minecraft/crash-reports/crash-2026-05-01_12.00.00-server.txt".to_owned())
        );

        // Enforce the 200-line tail window: 199 trailing lines still match, 200 trailing lines evict it.
        let mut stdout = Buffer::default();
        stdout.push(
            b"[12:00:00] [Server Watchdog/FATAL]: This crash report has been saved to: crash-reports/old.txt\n"
                .to_vec(),
        );
        for _ in 0..199 {
            stdout.push(b"noise line\n".to_vec());
        }
        assert_eq!(
            detect_crash_report(&stdout, &stderr, wd),
            Some("/srv/minecraft/crash-reports/old.txt".to_owned())
        );
        stdout.push(b"one more noise line\n".to_vec());
        assert_eq!(detect_crash_report(&stdout, &stderr, wd), None);
    }
}
