//! Shared Clap options and typed parsing for each executable's command root.

use std::ffi::OsString;

use clap::{ArgMatches, Args, ColorChoice, CommandFactory, FromArgMatches, Parser};

use crate::{commands::Command, program::Program, ssh::SshArgs};

mod roots;
pub(crate) mod source;

/// Parses the actual root directly; no synthetic command components enter argv.
pub(crate) fn parse(program: Program, args: Vec<OsString>, color: ColorChoice) -> Invocation {
    let mut command = command(program, color);
    let matches = command
        .try_get_matches_from_mut(args)
        .unwrap_or_else(|error| error.exit());
    match program {
        Program::Mcpm | Program::McpmServer => parse_root::<roots::ServerRootArgs>(&matches),
        Program::Mcpmc | Program::McpmClient => parse_root::<roots::ClientRootArgs>(&matches),
        Program::McpmTools => parse_root::<roots::ToolsRootArgs>(&matches),
    }
}

/// The same native command schema used for parsing and recursive help output.
pub(crate) fn command(program: Program, color: ColorChoice) -> clap::Command {
    let root = match program {
        Program::Mcpm | Program::McpmServer => root_command::<roots::ServerRootArgs>(),
        Program::Mcpmc | Program::McpmClient => root_command::<roots::ClientRootArgs>(),
        Program::McpmTools => root_command::<roots::ToolsRootArgs>(),
    };
    root.name(program.name())
        .bin_name(program.name())
        .color(color)
}

fn root_command<R: Root>() -> clap::Command {
    Cli::<R>::command().about(R::ABOUT)
}

fn parse_root<R: Root>(matches: &ArgMatches) -> Invocation {
    let cli = Cli::<R>::from_arg_matches(matches).unwrap_or_else(|error| error.exit());
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
