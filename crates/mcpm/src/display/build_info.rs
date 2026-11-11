//! Startup notice rendering for builds outside the official release workflow.

use std::io;

use mcpm_rich::rich;

use super::{console::ConsoleWriter, rich::block, style::OutputStyle};

pub(crate) fn unofficial_warning(output: &mut ConsoleWriter<'_>) -> io::Result<()> {
    let warning = rich!(
        "[warning:](warning) This is not an official mcpm release build; it may be a third-party
         build or contain changes that have not been officially released."
    );
    block(output, &warning, &OutputStyle::stderr())
}
