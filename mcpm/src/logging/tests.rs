//! Verify independent sink filtering and file-only capture of user-facing error chains.

use std::fs;

use super::*;

#[test]
fn file_keeps_debug_and_errors_independently_of_console_level() {
    for level in [LogLevel::Off, LogLevel::Warn, LogLevel::Trace] {
        let console = tempfile::NamedTempFile::new().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        tracing::subscriber::with_default(
            subscriber(
                level,
                console.reopen().unwrap(),
                file.reopen().unwrap(),
                false,
            ),
            || {
                tracing::debug!(value = 42, "debug detail");
                tracing::warn!("warning detail");
                tracing::trace!("trace detail");
                record_error(&anyhow::anyhow!("root failure").context("outer context"));
            },
        );
        let file = fs::read_to_string(file.path()).unwrap();
        assert!(
            file.contains("DEBUG") && file.contains("debug detail") && file.contains("value=42")
        );
        assert!(file.contains("WARN") && file.contains("warning detail"));
        assert!(file.contains("ERROR") && file.contains("outer context: root failure"));
        assert!(!file.contains("trace detail"));
        assert!(!file.contains('\u{1b}'));
        let console = fs::read_to_string(console.path()).unwrap();
        assert!(!console.contains("outer context"));
        match level {
            LogLevel::Off => assert!(console.is_empty()),
            LogLevel::Warn => {
                assert!(console.contains("warning detail"));
                assert!(!console.contains("debug detail"));
                assert!(!console.contains("trace detail"));
            }
            LogLevel::Trace => {
                assert!(console.contains("trace detail") && console.contains("debug detail"))
            }
            _ => unreachable!(),
        }
    }
}
