//! Line protocol shared by instance clients and the shim.

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use super::state::InstanceState;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Hello {
        tail: usize,
    },
    Started {
        state: InstanceState,
    },
    Line {
        sequence: u64,
        stream: String,
        text: String,
    },
    Missing {
        count: u64,
    },
    Input {
        line: String,
    },
    Stop {
        force: bool,
    },
    Exit {
        code: Option<i32>,
        signal: Option<i32>,
        stopped: bool,
    },
    Error {
        message: String,
    },
    Bye,
}

pub fn write<W: Write>(output: &mut W, frame: &Frame) -> io::Result<()> {
    serde_json::to_writer(&mut *output, frame).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

pub fn read<R: BufRead>(input: &mut R) -> io::Result<Option<Frame>> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    serde_json::from_str(line.trim_end())
        .map(Some)
        .map_err(io::Error::other)
}
