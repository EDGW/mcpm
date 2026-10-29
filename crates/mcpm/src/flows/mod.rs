//! Operation pipelines shared by commands and interactive views.
//!
//! A flow only starts operations, forwards typed events and calls backend helpers between
//! steps; it never builds a provider, touches `data/` or implements a business rule.

pub(crate) mod cache_ident_refresh;
pub(crate) mod identify;
