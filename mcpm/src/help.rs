//! Orders native Clap help sections after inherited global arguments are available.

use clap::{Arg, ArgAction, Command};

/// Run through `Command::defer`, before Clap builds its argument lookup tables.
/// Argument definitions and Clap's own help rendering remain authoritative.
pub(crate) fn grouped(command: Command, headings: &[&'static str]) -> Command {
    let command = command.disable_help_flag(true).arg(
        Arg::new("help")
            .short('h')
            .long("help")
            .action(ArgAction::Help)
            .help("Print help")
            .long_help("Print help (see a summary with '-h')")
            .help_heading(headings.first().copied()),
    );
    let mut args: Vec<_> = command.get_arguments().cloned().collect();
    args.sort_by_key(|arg| {
        headings
            .iter()
            .position(|heading| Some(*heading) == arg.get_help_heading())
            .unwrap_or(headings.len())
    });
    let mut args = args.into_iter();
    command.mut_args(|_| args.next().expect("one ordered entry per argument"))
}
