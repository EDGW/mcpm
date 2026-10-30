//! Serialized line input to the Java process.

use std::io::Write;

pub fn send(stdin: &mut impl Write, line: &str) -> std::io::Result<()> {
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}
