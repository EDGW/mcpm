//! Frontend names for backend selection types; Clap parsing stays in `args`.

mod args;

pub(in crate::commands) use args::{ItemArgs, SelectionArgs};
pub(in crate::commands) use mcpm_backend::mods::selection::{
    FileScope as Scope, Query, QueryKind, Selection, SelectedFile,
};
