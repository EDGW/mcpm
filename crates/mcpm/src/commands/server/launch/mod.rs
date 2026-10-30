//! Server instance launch through the host operation and detached shim.
// signal and foreground tty handling intentionally do not exist; Java is owned by shim.
use crate::{
    commands::{CommandOutcome, execution},
    ssh::SshArgs,
};
use anyhow::Result;
use clap::{Args, ValueEnum};
use mcpm_backend::data::server::authlib::JarPolicy;
use mcpm_backend::server::instance::{
    Attach, AttachEvent, AttachInput, ConfigurationOverridesWire, Send, SendInput, Start,
    StartInput,
};
use std::io::{self, BufRead, IsTerminal};
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(
    after_help = "Modes: Console and Raw. Launch starts the detached server shim; use server attach to observe it."
)]
pub(super) struct LaunchArgs {
    #[arg(long, value_name = "PATH", default_value = ".")]
    server: PathBuf,
    #[arg(short = 'O', long = "override", value_name = "KEY=VALUE")]
    overrides: Vec<String>,
    #[arg(long, value_name = "EXECUTABLE")]
    java: Option<String>,
    #[arg(short = 'm', long, value_name = "[INITIAL:]MAXIMUM")]
    memory: Option<String>,
    #[arg(short = 'G', long, value_name = "COLLECTOR")]
    gc: Option<String>,
    #[arg(long, value_name = "PORT")]
    port: Option<u16>,
    #[arg(long, value_name = "DIRECTORY")]
    world: Option<String>,
    #[arg(short = 'A', long, value_name = "API_URL")]
    auth: Option<String>,
    #[arg(long, value_enum, value_name = "POLICY")]
    auth_jar_policy: Option<AuthJarPolicy>,
    #[arg(short = 'E', long, value_name = "ARGUMENT", allow_hyphen_values = true)]
    extra: Option<String>,
    #[arg(long)]
    detach: bool,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum AuthJarPolicy {
    Auto,
    Local,
    Refresh,
}
impl From<AuthJarPolicy> for JarPolicy {
    fn from(v: AuthJarPolicy) -> Self {
        match v {
            AuthJarPolicy::Auto => Self::Auto,
            AuthJarPolicy::Local => Self::Local,
            AuthJarPolicy::Refresh => Self::Refresh,
        }
    }
}
impl LaunchArgs {
    fn assignments(&self) -> Vec<String> {
        let mut a = self.overrides.clone();
        if let Some(v) = &self.java {
            a.push(format!("java.executable={v}"));
        }
        if let Some(v) = &self.memory {
            a.push(format!("jvm.heap.maximum={v}"));
        }
        if let Some(v) = &self.gc {
            a.push(format!("jvm.gc.collector={v}"));
        }
        if let Some(v) = self.port {
            a.push(format!("server.port={v}"));
        }
        if let Some(v) = &self.world {
            a.push(format!("server.world={v}"));
        }
        if let Some(v) = &self.auth {
            a.push(format!("authlib-injector.apiUrl={v}"));
        }
        if let Some(v) = &self.extra {
            a.push(format!("jvm.extraArguments={v}"));
        }
        a
    }
}
pub(super) fn run(args: LaunchArgs, ssh: SshArgs) -> Result<CommandOutcome> {
    let runtime = execution::Runtime::new()?;
    let session = execution::session_for(&ssh)?;
    let input = StartInput {
        path: args.server.clone(),
        overrides: ConfigurationOverridesWire {
            assignments: args.assignments(),
        },
        auth_api: args.auth.clone(),
        auth_jar_policy: args.auth_jar_policy.map(Into::into),
        stop_timeout_secs: 10,
    };
    let handle = runtime
        .block_on(session.start::<Start>(input))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    let output = runtime
        .block_on(handle.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    println!("{}\t{:?}", output.instance_id, output.status);
    if args.detach {
        return Ok(CommandOutcome::Completed);
    }
    let mut attach = runtime
        .block_on(session.start::<Attach>(AttachInput {
            path: args.server.clone(),
            tail: 0,
        }))
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    if !io::stdin().is_terminal() {
        let path = args.server.clone();
        for line in io::stdin().lock().lines() {
            let send = runtime
                .block_on(session.start::<Send>(SendInput {
                    path: path.clone(),
                    line: line?,
                }))
                .map_err(|error| execution::failure(error, ssh.is_requested()))?;
            runtime
                .block_on(send.finish())
                .map_err(|error| execution::failure(error, ssh.is_requested()))?;
        }
    }
    while let Some(AttachEvent { text, .. }) = runtime.block_on(attach.next()) {
        println!("{text}");
    }
    runtime
        .block_on(attach.finish())
        .map_err(|error| execution::failure(error, ssh.is_requested()))?;
    Ok(CommandOutcome::Completed)
}
