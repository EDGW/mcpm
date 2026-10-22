//! String command ID registration through module initcalls; payload decoding belongs to handlers.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use anyhow::{Result, ensure};
use serde_json::Value;

use super::command::{Command, Output};

type Handler = fn(Value, &Output) -> Result<()>;

#[derive(Clone, Default, Debug)]
pub(crate) struct Registry {
    handlers: BTreeMap<&'static str, Handler>,
}

impl Registry {
    pub(crate) fn register<C: Command>(&mut self) -> Result<()> {
        ensure!(!C::ID.is_empty(), "remote command ID must not be empty");
        ensure!(
            !self.handlers.contains_key(C::ID),
            "duplicate remote command ID: {}",
            C::ID
        );
        self.handlers.insert(C::ID, C::run);
        tracing::debug!(command = C::ID, "Registered remote command");
        Ok(())
    }

    pub(super) fn ids(&self) -> Vec<String> {
        self.handlers.keys().map(|id| (*id).to_owned()).collect()
    }
    pub(super) fn handler(&self, id: &str) -> Option<Handler> {
        self.handlers.get(id).copied()
    }
}

static REGISTERED: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));

pub(crate) fn register<C: Command>() -> Result<()> {
    REGISTERED.lock().unwrap().register::<C>()
}
pub(crate) fn registered() -> Registry {
    REGISTERED.lock().unwrap().clone()
}
