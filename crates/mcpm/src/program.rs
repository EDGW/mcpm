//! Executable identities and immutable frontend command roots, independent of command implementations.

use std::{fmt, sync::OnceLock};

/// Selects an executable's name and command family without inspecting its filename.
#[derive(Clone, Copy, Debug)]
pub enum Program {
    Mcpm,
    McpmServer,
    Mcpmc,
    McpmClient,
    McpmTools,
}

impl Program {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Mcpm => "mcpm",
            Self::McpmServer => "mcpm-server",
            Self::Mcpmc => "mcpmc",
            Self::McpmClient => "mcpm-client",
            Self::McpmTools => "mcpm-tools",
        }
    }

    pub(crate) fn roots(self) -> CommandRoots {
        let server = match self {
            Self::McpmServer => CommandRoot::new(self, &[]),
            Self::McpmTools => CommandRoot::new(self, &["server"]),
            _ => CommandRoot::new(Self::Mcpm, &[]),
        };
        let client = match self {
            Self::McpmClient | Self::Mcpmc => CommandRoot::new(self, &[]),
            Self::McpmTools => CommandRoot::new(self, &["client"]),
            _ => CommandRoot::new(Self::Mcpmc, &[]),
        };
        CommandRoots {
            server,
            client,
            tools: CommandRoot::new(Self::McpmTools, &[]),
        }
    }
}

/// All frontend command roots for one process; initialized before Clap builds help.
#[derive(Debug)]
pub(crate) struct CommandRoots {
    pub(crate) server: CommandRoot,
    pub(crate) client: CommandRoot,
    pub(crate) tools: CommandRoot,
}

impl CommandRoots {
    pub(crate) fn init(program: Program) {
        ROOTS
            .set(program.roots())
            .expect("command roots must be initialized once");
    }
}

/// A program and its command components, rendered only when constructing frontend text.
#[derive(Debug)]
pub(crate) struct CommandRoot {
    program: Program,
    components: &'static [&'static str],
}

impl CommandRoot {
    const fn new(program: Program, components: &'static [&'static str]) -> Self {
        Self {
            program,
            components,
        }
    }

    pub(crate) fn command(&self, suffix: &str) -> String {
        format!("{self} {suffix}")
    }
}

impl fmt::Display for CommandRoot {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.program.name())?;
        for component in self.components {
            write!(output, " {component}")?;
        }
        Ok(())
    }
}

static ROOTS: OnceLock<CommandRoots> = OnceLock::new();

pub(crate) fn roots() -> &'static CommandRoots {
    ROOTS
        .get()
        .expect("command roots must be initialized before use")
}
