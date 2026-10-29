//! Immutable build identity and detailed version metadata shared by all executables.

pub(crate) const VERSION: &str = env!("MCPM_BUILD_VERSION");

pub(crate) const LONG_VERSION: &str = concat!(
    env!("MCPM_BUILD_VERSION"),
    "\nBuild: ",
    env!("MCPM_BUILD_CHANNEL"),
    " (",
    env!("MCPM_BUILD_PROFILE"),
    ")\nCommit: ",
    env!("MCPM_BUILD_COMMIT"),
    "\n\nContributors:\n",
    include_str!(concat!(env!("OUT_DIR"), "/contributors.txt"))
);

/// Debug builds expose their identity in version details without a startup warning.
pub(crate) fn is_unofficial_release() -> bool {
    env!("MCPM_BUILD_PROFILE") == "release" && env!("MCPM_BUILD_CHANNEL") == "unofficial"
}
