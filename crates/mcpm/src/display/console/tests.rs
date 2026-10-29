//! Stream behavior across nested writers, fragmented writes and short underlying writes.

use std::io::{self, Write};

use super::ConsoleWriter;

#[test]
fn nested_writers_share_line_state_and_leave_blank_lines_empty() -> io::Result<()> {
    let mut bytes = Vec::new();
    let mut root = ConsoleWriter::new(&mut bytes);
    write!(root, "root")?;
    {
        let mut child = root.subwriter();
        child.write_all(b" continued\n\n")?;
        write!(child, "child ")?;
        {
            let mut grandchild = child.subwriter();
            grandchild.write_all("继续\ngrandchild\n\n".as_bytes())?;
        }
        writeln!(child, "sibling")?;
        write!(child, "partial")?;
    }
    writeln!(root, " end")?;
    writeln!(root.subwriter(), "next child")?;
    writeln!(root, "root again")?;
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "root continued\n\n  child 继续\n    grandchild\n\n  sibling\n  partial end\n  next child\nroot again\n"
    );
    Ok(())
}

#[test]
fn short_writes_and_interrupted_output_do_not_repeat_indentation() -> io::Result<()> {
    struct ShortWriter {
        bytes: Vec<u8>,
        interrupted: bool,
        flushed: bool,
    }

    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let end = bytes.len().min(1);
            self.bytes.extend_from_slice(&bytes[..end]);
            Ok(end)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            Ok(())
        }
    }

    let mut output = ShortWriter {
        bytes: Vec::new(),
        interrupted: false,
        flushed: false,
    };
    let mut root = ConsoleWriter::new(&mut output);
    let mut child = root.subwriter();
    child.subwriter().write_all("一\ntwo\n\n".as_bytes())?;
    child.flush()?;
    assert_eq!(output.bytes, "    一\n    two\n\n".as_bytes());
    assert!(output.flushed);
    Ok(())
}
