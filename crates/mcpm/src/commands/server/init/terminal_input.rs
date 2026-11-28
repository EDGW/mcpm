//! Reads cancellable Console prompt input from the controlling terminal.

use std::io;

use console::{Key, Term};
#[cfg(windows)]
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};

/// Reads and echoes the accepted characters until Enter or cancellation.
pub(super) fn read_line(term: &Term, accept: impl Fn(char) -> bool) -> io::Result<Option<String>> {
    let mut answer = String::new();
    #[cfg(unix)]
    let mut input = ConsoleInput::open()?;
    loop {
        #[cfg(unix)]
        let key = input.read_key();
        #[cfg(windows)]
        let key = read_key();
        match key {
            Ok(Key::Enter) => {
                term.write_line("")?;
                return Ok(Some(answer));
            }
            Ok(Key::Escape | Key::CtrlC) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::Interrupted
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
            Ok(Key::Char(ch)) if !ch.is_control() && accept(ch) => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            Ok(Key::Backspace) if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
    term.write_line("")?;
    Ok(None)
}

/// Read only /dev/tty, even when stdin points at another terminal or a pipe.
#[cfg(unix)]
struct ConsoleInput {
    input: std::fs::File,
    original: libc::termios,
}
#[cfg(unix)]
impl ConsoleInput {
    fn open() -> io::Result<Self> {
        use std::os::fd::AsRawFd;
        let input = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")?;
        let mut original = std::mem::MaybeUninit::uninit();
        // SAFETY: input is a live terminal descriptor; tcgetattr initializes on success.
        if unsafe { libc::tcgetattr(input.as_raw_fd(), original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let original = unsafe { original.assume_init() };
        let mut mode = original;
        mode.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        mode.c_cc[libc::VMIN] = 1;
        mode.c_cc[libc::VTIME] = 0;
        // SAFETY: mode is initialized and the descriptor remains owned by input.
        if unsafe { libc::tcsetattr(input.as_raw_fd(), libc::TCSANOW, &mode) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { input, original })
    }

    fn read_key(&mut self) -> io::Result<Key> {
        use std::io::Read;
        let mut byte = [0];
        if self.input.read(&mut byte)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(match byte[0] {
            b'\r' | b'\n' => Key::Enter,
            0x1b => Key::Escape,
            0x03 | 0x04 | 0x1a => Key::CtrlC,
            0x08 | 0x7f => Key::Backspace,
            value if value.is_ascii() => Key::Char(char::from(value)),
            _ => Key::Unknown,
        })
    }
}
#[cfg(unix)]
impl Drop for ConsoleInput {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is still owned and original came from tcgetattr.
        unsafe {
            libc::tcsetattr(self.input.as_raw_fd(), libc::TCSANOW, &self.original);
        }
    }
}

/// Use CONIN$ on Windows, even if stdin was redirected. Restore raw mode before echoing.
#[cfg(windows)]
fn read_key() -> io::Result<Key> {
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    terminal::enable_raw_mode()?;
    let _raw = Raw;
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        return Ok(match key.code {
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Escape,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Char('c' | 'd' | 'z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Key::Escape
            }
            KeyCode::Char(ch) => Key::Char(ch),
            _ => Key::Unknown,
        });
    }
}
