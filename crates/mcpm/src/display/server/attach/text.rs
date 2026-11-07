//! Incremental UTF-8/control decoding and bounded logical lines, independent of backend log transport.

use anyhow::Result;

#[derive(Default)]
struct Text(String);
impl vte::Perform for Text {
    fn print(&mut self, ch: char) {
        self.0.push(ch);
    }
    fn execute(&mut self, byte: u8) {
        if matches!(byte, b'\n' | b'\r' | b'\t' | 8) {
            self.0.push(byte as char);
        }
    }
}
#[derive(Default)]
pub(super) struct Stream {
    parser: vte::Parser,
    partial: String,
    first: bool,
    started: bool,
    carriage_return: bool,
}
impl Stream {
    pub fn feed(
        &mut self,
        bytes: &[u8],
        mut emit: impl FnMut(&str, bool, bool) -> Result<()>,
    ) -> Result<()> {
        let mut text = Text::default();
        self.parser.advance(&mut text, bytes);
        if !self.started {
            self.first = true;
            self.started = true;
        }
        for ch in text.0.chars() {
            if self.carriage_return && ch != '\n' && ch != '\r' {
                self.partial.clear();
            }
            self.carriage_return = ch == '\r';
            match ch {
                '\n' => {
                    emit(&self.partial, self.first, true)?;
                    self.partial.clear();
                    self.first = true;
                }
                '\r' => {}
                '\u{8}' => {
                    self.partial.pop();
                }
                '\t' => self.partial.push_str("    "),
                ch => self.partial.push(ch),
            }
            if self.partial.len() >= 8192 {
                emit(&self.partial, self.first, false)?;
                self.partial.clear();
                self.first = false;
            }
        }
        Ok(())
    }
    pub fn finish(&mut self, emit: impl FnOnce(&str, bool, bool) -> Result<()>) -> Result<()> {
        let mut tail = Text::default();
        // Resolve a truncated UTF-8 code point at the channel boundary without losing its bytes in Raw.
        self.parser.advance(&mut tail, b"\0");
        self.partial.push_str(&tail.0);
        if !self.partial.is_empty() {
            emit(&self.partial, self.first, false)?;
            self.partial.clear();
        }
        Ok(())
    }
    pub fn partial(&self) -> &str {
        &self.partial
    }
}
pub(super) fn clean(text: &str) -> String {
    let mut result = Text::default();
    let mut parser = vte::Parser::new();
    parser.advance(&mut result, text.as_bytes());
    result.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragments_and_crlf_preserve_one_logical_prefix() -> Result<()> {
        let mut stream = Stream::default();
        let mut lines = Vec::new();
        let mut collect = |text: &str, first, newline| {
            lines.push((text.to_owned(), first, newline));
            Ok(())
        };
        for bytes in "[12:34:56] [INFO] 中文\r\npartial".as_bytes().chunks(1) {
            stream.feed(bytes, &mut collect)?;
        }
        stream.finish(&mut collect)?;
        assert_eq!(
            lines,
            vec![
                ("[12:34:56] [INFO] 中文".into(), true, true),
                ("partial".into(), true, false)
            ]
        );
        Ok(())
    }
    #[test]
    fn long_continuations_never_become_new_prefixes() -> Result<()> {
        let mut stream = Stream::default();
        let mut lines = Vec::new();
        let text = format!("{}[12:34:56] [ERROR] body\n", "x".repeat(8192));
        stream.feed(text.as_bytes(), |text, first, newline| {
            lines.push((text.to_owned(), first, newline));
            Ok(())
        })?;
        assert!(lines[0].1);
        assert!(!lines[1].1);
        assert_eq!(
            lines
                .iter()
                .map(|(text, _, _)| text.as_str())
                .collect::<String>(),
            text.trim_end_matches('\n')
        );
        Ok(())
    }
}
