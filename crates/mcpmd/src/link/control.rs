//! Link lifecycle state and endpoint-local control operations.

use anyhow::{Context, Result};
use mcpm_exec::{
    operation::{Operation, Registry},
    service::control::{HelloInfo, LinkStatus, StopInput, VERSION},
};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Instant;
use tokio::sync::Notify;

static STATE: OnceLock<Arc<State>> = OnceLock::new();

pub struct State {
    pub clients: AtomicUsize,
    pub calls: AtomicUsize,
    pub stopping: AtomicBool,
    pub force: AtomicBool,
    pub ssh_alive: AtomicBool,
    pub changed: Notify,
    idle: Mutex<Option<Instant>>,
}
impl State {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            clients: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
            force: AtomicBool::new(false),
            ssh_alive: AtomicBool::new(true),
            changed: Notify::new(),
            idle: Mutex::new(Some(Instant::now())),
        })
    }
    pub fn lease(self: &Arc<Self>, call: bool) -> Lease {
        if call {
            self.calls.fetch_add(1, Ordering::SeqCst);
        } else {
            self.clients.fetch_add(1, Ordering::SeqCst);
        }
        *self.idle.lock().unwrap() = None;
        self.changed.notify_one();
        Lease {
            state: self.clone(),
            call,
        }
    }
    pub fn idle_since(&self) -> Option<Instant> {
        *self.idle.lock().unwrap()
    }
    pub fn stop(&self, force: bool) {
        self.stopping.store(true, Ordering::SeqCst);
        if force {
            self.force.store(true, Ordering::SeqCst);
        }
        self.changed.notify_one();
    }
}

pub struct Lease {
    state: Arc<State>,
    call: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.call {
            self.state.calls.fetch_sub(1, Ordering::SeqCst);
        } else {
            self.state.clients.fetch_sub(1, Ordering::SeqCst);
        }
        let mut idle = self.state.idle.lock().unwrap();
        if self.state.clients.load(Ordering::SeqCst) == 0
            && self.state.calls.load(Ordering::SeqCst) == 0
        {
            *idle = Some(Instant::now());
        }
        self.state.changed.notify_one();
    }
}

pub fn registry(state: Arc<State>) -> Result<Registry> {
    STATE
        .set(state)
        .map_err(|_| anyhow::anyhow!("link controls already initialized"))?;
    let mut registry = Registry::new();
    registry.register::<Hello>()?;
    registry.register::<Status>()?;
    registry.register::<Stop>()?;
    Ok(registry)
}
fn state() -> Result<&'static Arc<State>> {
    STATE
        .get()
        .context("link controls are unavailable outside a link")
}

struct Hello;
impl Operation for Hello {
    const ID: &'static str = "link.hello";
    type Input = ();
    type Output = HelloInfo;
    async fn run(_: ()) -> Result<HelloInfo> {
        Ok(HelloInfo {
            version: VERSION.into(),
            operations: ["link.hello", "link.status", "link.stop"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        })
    }
}
struct Status;
impl Operation for Status {
    const ID: &'static str = "link.status";
    type Input = ();
    type Output = LinkStatus;
    async fn run(_: ()) -> Result<LinkStatus> {
        let state = state()?;
        Ok(LinkStatus {
            connections: state.clients.load(Ordering::SeqCst),
            calls: state.calls.load(Ordering::SeqCst),
            idle_seconds: state
                .idle_since()
                .map(|time| time.elapsed().as_secs())
                .unwrap_or(0),
            ssh_alive: state.ssh_alive.load(Ordering::SeqCst),
        })
    }
}
struct Stop;
impl Operation for Stop {
    const ID: &'static str = "link.stop";
    type Input = StopInput;
    type Output = ();
    async fn run(input: StopInput) -> Result<()> {
        state()?.stop(input.force);
        Ok(())
    }
}
