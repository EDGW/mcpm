//! mcpm's own data: the sole codecs for what mcpm stores under `.mcpm/`, and the
//! resources that interpret it.
//!
//! Every mcpm-specific format is encoded and decoded in exactly one module here;
//! general Minecraft behaviour stays in `kako-mc-lib`.

pub mod data;
