//! Shared Clap options and typed parsing for each executable's command root.

use std::ffi::OsString;

use clap::{Args, ColorChoice, CommandFactory, FromArgMatches, Parser};

use crate::{commands::Command, program::Program, ssh::SshArgs};

mod roots;

/// Parses the actual root directly; no synthetic command components enter argv.
pub(crate) fn parse(program: Program, args: Vec<OsString>, color: ColorChoice) -> Invocation {
    match program {
        Program::Mcpm | Program::McpmServer => {
            parse_root::<roots::ServerRootArgs>(program, args, color)
        }
        Program::Mcpmc | Program::McpmClient => {
            parse_root::<roots::ClientRootArgs>(program, args, color)
        }
        Program::McpmTools => parse_root::<roots::ToolsRootArgs>(program, args, color),
    }
}

fn parse_root<R: Root>(program: Program, args: Vec<OsString>, color: ColorChoice) -> Invocation {
    let mut command = Cli::<R>::command()
        .name(program.name())
        .bin_name(program.name())
        .about(R::ABOUT)
        .color(color);
    let matches = command
        .try_get_matches_from_mut(args)
        .unwrap_or_else(|error| error.exit());
    let cli = Cli::<R>::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    cli.root.into_invocation(cli.common)
}

#[derive(Parser, Debug)]
#[command(version = crate::build_info::VERSION, long_version = crate::build_info::LONG_VERSION)]
struct Cli<R: Args> {
    #[command(flatten)]
    common: CommonArgs,
    #[command(flatten)]
    root: R,
}

/// Options shared by every executable and inherited by every command.
#[derive(Args, Debug)]
pub(crate) struct CommonArgs {
    /// Disable TUI interaction, formatting, colors, progress, and hints; otherwise each output stream follows its own TTY status.
    #[arg(long, global = true, help_heading = "Mode options")]
    pub(crate) raw: bool,
    /// Diagnostic log level on stderr; does not filter game stdout/stderr.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "warn",
        help_heading = "Global options"
    )]
    pub(crate) log_level: crate::logging::LogLevel,
}

/// Remote log presentation, available to the server and tools roots.
#[derive(Args, Debug)]
struct RemoteLogArgs {
    /// Show remote logs on stderr, filtered by --log-level; remote errors are always shown.
    #[arg(long, global = true, help_heading = "Global options")]
    remote_logs: bool,
}

/// Converts root-specific options into one startup invocation.
trait Root: Args {
    const ABOUT: &'static str;
    fn into_invocation(self, common: CommonArgs) -> Invocation;
}

/// Normalized startup options; command schemas remain owned by their command modules.
pub(crate) struct Invocation {
    pub(crate) common: CommonArgs,
    pub(crate) command: Command,
    pub(crate) ssh: SshArgs,
    pub(crate) remote_logs: bool,
}
