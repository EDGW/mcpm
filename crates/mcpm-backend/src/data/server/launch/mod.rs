//! Launch plans derived from `launch.toml` configuration and their foreground execution.

mod plan;
pub use plan::{LaunchPlan, prepare, prepare_with_overrides, prepare_with_progress};

mod progress;
pub use progress::{Event, Stage};

mod process;
pub use process::{ForegroundStreams, LaunchExit, StopRequest, execute};
