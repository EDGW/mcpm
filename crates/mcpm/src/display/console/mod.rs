//! Streaming Console output with borrowed subwriters and a shared indentation policy.

use std::io::{self, Write};

/// One Console nesting level. Renderers create subwriters instead of spelling spaces.
const INDENT: &str = "  ";

/// Each subwriter borrows its parent, so line state stays current across nested writes.
pub(crate) struct ConsoleWriter<'a> {
    output: &'a mut dyn Write,
    nested: bool,
    line_start: bool,
}

impl<'a> ConsoleWriter<'a> {
    pub(crate) fn new(output: &'a mut dyn Write) -> Self {
        Self {
            output,
            nested: false,
            line_start: true,
        }
    }

    /// Adds one level on subsequent nonempty lines; a partial line stays continuous.
    pub(crate) fn subwriter(&mut self) -> ConsoleWriter<'_> {
        let line_start = self.line_start;
        ConsoleWriter {
            output: self,
            nested: true,
            line_start,
        }
    }
}

impl Write for ConsoleWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.line_start && buffer[0] != b'\n' {
            if self.nested {
                self.output.write_all(INDENT.as_bytes())?;
            }
            self.line_start = false;
        }
        // Forward one line fragment at a time, preserving the underlying Write contract.
        let end = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        let written = self.output.write(&buffer[..end])?;
        if written > 0 {
            self.line_start = buffer[written - 1] == b'\n';
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

#[cfg(test)]
mod tests;
