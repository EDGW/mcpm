//! Remote mod source interfaces and shared project models.

pub mod curseforge;
pub mod modrinth;

mod file;
pub use file::*;

mod project;
pub use project::*;

mod interface;
pub use interface::*;
