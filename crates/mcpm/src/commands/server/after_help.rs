//! Styled after-help text for server launch and attach.
//!
//! The text is user-facing: internal frame, session, and shim terminology stays out.

use clap::builder::StyledStr;
use owo_colors::OwoColorize;

/// Builds the `launch` after-help text.
pub(super) fn launch() -> StyledStr {
    let mut help = Help::new();
    help.heading("Modes");
    help.blank();
    help.line(&format!("  {} (terminal default)", bold("Inline TUI")));
    help.line("    Logs scroll above the command area;");
    help.line(&format!(
        "    {}+Up/Down adjusts its height.",
        crate::display::server::attach::modifier_name()
    ));
    help.line("    Timestamps and log bodies through WARN are muted;");
    help.line("    level markers are colored.");
    help.blank();
    help.line(&format!("  {} ({})", bold("Console"), bold("--no-ui")));
    help.line("    Use ordinary console output without the inline interface.");
    help.blank();
    help.line(&format!(
        "  {} ({} or redirected stdout)",
        bold("Raw"),
        bold("--raw")
    ));
    help.line("    Disable TUI interaction, formatting, colors, progress, and hints.");

    help.blank();
    help.heading("Process");
    help.blank();
    help.line("  Java runs as an independent process, locally or on the selected");
    help.line("  SSH/Remote host.");
    help.line(&format!(
        "  {}: return as soon as Java starts and print its run id;",
        bold("--detach")
    ));
    help.line("  Java keeps running.");
    help.line(&format!(
        "  {}: choose how commands reach this Java instance:",
        bold("--communication")
    ));
    help.line("  auto, RCON, or stdin.");
    help.line("  No automatic kill timeout is applied.");

    help.blank();
    help.heading("Commands");
    help.blank();
    help.line("  Every attached client can send commands and receive live results.");
    help.line("  Your own replies appear as usual; replies from another client are labeled");
    help.line("  \"Executed by another Listener\".");
    help.blank();
    help.line(&format!(
        "  {} / EOF / Ctrl+D: detach without stopping Java.",
        bold("@detach")
    ));
    help.line(&format!(
        "  {}: send stop without escalation.",
        bold("@stop")
    ));
    help.line(&format!("  {}: force-kill Java.", bold("@kill")));
    help.line(&format!(
        "  {}: switch the speed row between compact 100ms averages (default)",
        bold("@speed")
    ));
    help.line("          and full 100ms/1000ms lifetime peaks and averages.");
    help.line(&format!(
        "  {}: send a built-in command literally.",
        bold("@@<command>")
    ));
    help.blank();
    help.line("  Ctrl+C sends stop; another Ctrl+C while stopping kills Java.");
    help.line(&format!(
        "  In {} / {} mode, {} prints a snapshot instead of updating a row.",
        bold("Console"),
        bold("Raw"),
        bold("@speed")
    ));
    help.line("  Local runs without traffic reporting show speed as unavailable.");

    help.blank();
    help.heading("Help and hints");
    help.blank();
    help.line("  When an attachment connects over RCON, it requests help once;");
    help.line("  the result stays local to that attachment.");
    help.line("  Hints appear for nonblank input, wrap long syntax, and show five candidates.");
    help.line("  Recognized gamerule syntax enables rule-name completion and reports");
    help.line("  invalid type or value without blocking submission.");
    help.line("  Built-in hints remain available even if help fails.");

    help.blank();
    help.heading("Command input");
    help.blank();
    help.line("  With empty input:");
    help.line("    Up/Down recalls commands submitted in this attachment.");
    help.line("    Arrows continue through recalled entries until editing.");
    help.line("    Down past the newest entry clears input.");
    help.blank();
    help.line("  Otherwise:");
    help.line("    Up/Down browses hints; Tab fills.");
    help.line("    Enter always sends the current input, including input with errors.");
    help.blank();
    help.line("  Leading spaces trigger a warning in the Command Input title:");
    help.line("  Minecraft may not recognize the command.");
    help.line("  Commands are never trimmed.");

    help.finish()
}

/// Builds the `attach` after-help text.
pub(super) fn attach() -> StyledStr {
    let mut help = Help::new();
    help.heading("Modes");
    help.blank();
    help.line(&format!("  {} (terminal default)", bold("Inline TUI")));
    help.line("    The inline input stays below ordinary scrollback output.");
    help.line(&format!(
        "    {}+Up/Down changes its height.",
        crate::display::server::attach::modifier_name()
    ));
    help.blank();
    help.line(&format!("  {} ({})", bold("Console"), bold("--no-ui")));
    help.line("    Use ordinary console output without the inline interface.");
    help.blank();
    help.line(&format!(
        "  {} ({} or redirected stdout)",
        bold("Raw"),
        bold("--raw")
    ));
    help.line("    Disable TUI interaction, formatting, colors, progress, and hints.");

    help.blank();
    help.heading("Attachment");
    help.blank();
    help.line("  Attach connects to an existing Java instance and never starts Java.");
    help.line("  Command history replay defaults to one terminal screen of lines.");
    help.line(&format!(
        "  {} changes the limit; zero disables replay.",
        bold("--history-lines")
    ));
    help.line("  Re-attaching does not replay old command results.");

    help.blank();
    help.heading("Commands");
    help.blank();
    help.line("  Every attached client can send commands and receive live command results.");
    help.line("  Your own replies appear as usual; replies from another client are labeled");
    help.line("  \"Executed by another Listener\".");
    help.blank();
    help.line(&format!("  {} / EOF / Ctrl+D: detach.", bold("@detach")));
    help.line(&format!(
        "  {}: send stop without escalation.",
        bold("@stop")
    ));
    help.line(&format!("  {}: force-kill Java.", bold("@kill")));
    help.line(&format!(
        "  {}: switch the speed row between compact 100ms averages (default)",
        bold("@speed")
    ));
    help.line("          and full 100ms/1000ms lifetime peaks and averages.");
    help.line(&format!(
        "  {}: send a built-in command literally.",
        bold("@@<command>")
    ));
    help.blank();
    help.line("  Ctrl+C sends stop; another Ctrl+C while stopping force-kills.");
    help.line(&format!(
        "  In {} / {} mode, {} prints a snapshot instead of updating a row.",
        bold("Console"),
        bold("Raw"),
        bold("@speed")
    ));
    help.line("  Local runs without traffic reporting show speed as unavailable.");

    help.blank();
    help.heading("Help and hints");
    help.blank();
    help.line("  When this attachment connects over RCON, it requests help once;");
    help.line("  the result stays local to that attachment.");
    help.line("  Hints appear for nonblank input, normally match command names,");
    help.line("  wrap long syntax, and display five candidates.");
    help.line("  Recognized gamerule syntax enables rule-name completion and reports");
    help.line("  invalid type or value without blocking submission.");
    help.line("  Built-in hints remain available even if help fails.");

    help.blank();
    help.heading("Command input");
    help.blank();
    help.line("  With empty input:");
    help.line("    Up/Down recalls commands submitted in this attachment.");
    help.line("    Arrows continue through recalled entries until editing.");
    help.line("    Down past the newest entry clears input.");
    help.blank();
    help.line("  Otherwise:");
    help.line("    Up/Down browses hints with a position counter.");
    help.line("    Tab fills.");
    help.line("    Enter always sends the current input, including input with errors.");
    help.blank();
    help.line("  Leading spaces trigger a warning in the Command Input title:");
    help.line("  Minecraft may not recognize the command.");
    help.line("  Commands are never trimmed.");

    help.finish()
}

/// Line-oriented builder that keeps styling out of each command's text flow.
struct Help(String);

impl Help {
    fn new() -> Self {
        Self(String::new())
    }

    fn heading(&mut self, text: &str) {
        self.line(&format!("{}", text.bold().underline()));
    }

    fn line(&mut self, text: &str) {
        self.0.push_str(text);
        self.0.push('\n');
    }

    fn blank(&mut self) {
        self.0.push('\n');
    }

    fn finish(mut self) -> StyledStr {
        while self.0.ends_with('\n') {
            self.0.pop();
        }
        StyledStr::from(self.0)
    }
}

fn bold(text: &str) -> String {
    text.bold().to_string()
}
