//! Independent server.init and server.init.recognize registration, typed requests and validated replies.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    model::{Candidate, Initialize, Initialized, Phase, Recognize},
    operation,
};
use crate::remote::{
    client::Session,
    command::{Command, Output},
};

struct Init;
impl Command for Init {
    const ID: &'static str = "server.init";
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Initialize = serde_json::from_value(contents)
            .context("invalid complete server initialization arguments")?;
        let result = operation::initialize(
            args,
            || output.checkpoint(),
            |phase| output.send(Event::Stage { phase }),
        )?;
        output.send(Event::Initialized(Box::new(result)))
    }
}

struct Recognition;
impl Command for Recognition {
    const ID: &'static str = "server.init.recognize";
    fn run(contents: Value, output: &Output) -> Result<()> {
        let args: Recognize =
            serde_json::from_value(contents).context("invalid server recognition arguments")?;
        // Remote execution never treats a Remote workspace as a Local target.
        // Recognition itself remains independent of server.json registration data.
        super::super::workspace::require_local(&args.path)?;
        let candidates = operation::recognize(
            args,
            || output.checkpoint(),
            |phase| output.send(Event::Stage { phase }),
        )?;
        output.send(Event::Candidates { candidates })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Stage { phase: Phase },
    Candidates { candidates: Vec<Candidate> },
    Initialized(Box<Initialized>),
}

pub(super) fn recognize(
    session: &Session,
    args: Recognize,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Vec<Candidate>> {
    match exchange(
        session,
        Recognition::ID,
        serde_json::to_value(args)?,
        Kind::Recognition,
        phase,
    )? {
        Event::Candidates { candidates } => Ok(candidates),
        _ => unreachable!("validated recognition result"),
    }
}

pub(super) fn initialize(
    session: &Session,
    args: Initialize,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Initialized> {
    match exchange(
        session,
        Init::ID,
        serde_json::to_value(args)?,
        Kind::Initialization,
        phase,
    )? {
        Event::Initialized(result) => Ok(*result),
        _ => unreachable!("validated initialization result"),
    }
}

#[tracing::instrument(level = "debug", skip_all, fields(command), err(level = "debug"))]
fn exchange(
    session: &Session,
    command: &str,
    contents: Value,
    kind: Kind,
    phase: impl Fn(Phase) -> Result<()>,
) -> Result<Event> {
    tracing::info!(command, "Requesting remote server initialization operation");
    let mut validator = Validator {
        kind,
        stage: 0,
        done: false,
    };
    let mut result = None;
    session.submit(command, contents)?.collect(|reply| {
        let event: Event = reply
            .decode()
            .context("invalid remote server initialization contents")?;
        validator.accept(&event)?;
        match event {
            Event::Stage { phase: current } => phase(current)?,
            completion => result = Some(completion),
        }
        Ok(())
    })?;
    ensure!(
        validator.done,
        "missing remote server initialization result"
    );
    tracing::debug!(command, "Remote server initialization operation completed");
    result.context("missing remote server initialization result")
}

#[derive(Clone, Copy)]
enum Kind {
    Recognition,
    Initialization,
}
impl Kind {
    fn phases(self) -> &'static [Phase] {
        match self {
            Self::Recognition => &[Phase::Recognizing],
            Self::Initialization => &[Phase::Validating, Phase::Writing],
        }
    }
}
struct Validator {
    kind: Kind,
    stage: usize,
    done: bool,
}
impl Validator {
    fn accept(&mut self, event: &Event) -> Result<()> {
        ensure!(!self.done, "server initialization data after completion");
        match event {
            Event::Stage { phase } => {
                ensure!(
                    self.kind.phases().get(self.stage) == Some(phase),
                    "invalid server initialization phase sequence"
                );
                self.stage += 1;
            }
            Event::Candidates { candidates } => {
                ensure!(
                    matches!(self.kind, Kind::Recognition),
                    "unexpected server recognition result"
                );
                ensure!(
                    !candidates.is_empty()
                        && candidates.iter().all(|candidate| {
                            candidate.line > 0
                                && !candidate.main_class.is_empty()
                                && candidate.metadata.start.is_some()
                        }),
                    "invalid remote server candidates"
                );
                self.complete()?;
            }
            Event::Initialized(_) => {
                ensure!(
                    matches!(self.kind, Kind::Initialization),
                    "unexpected server initialization result"
                );
                self.complete()?;
            }
        }
        Ok(())
    }
    fn complete(&mut self) -> Result<()> {
        ensure!(
            self.stage == self.kind.phases().len(),
            "server initialization result before phases complete"
        );
        self.done = true;
        Ok(())
    }
}

fn init() -> Result<()> {
    crate::remote::registry::register::<Init>()?;
    crate::remote::registry::register::<Recognition>()
}
inventory::submit! { crate::initcall::Initcall(init) }

#[cfg(test)]
mod tests {
    use super::*;

    fn validator(kind: Kind) -> Validator {
        Validator {
            kind,
            stage: 0,
            done: false,
        }
    }
    fn stage(phase: Phase) -> Event {
        Event::Stage { phase }
    }
    fn initialized() -> Event {
        Event::Initialized(Box::new(Initialized { server: serde_json::from_value(serde_json::json!({
            "name":"server", "path":"C:\\server", "minecraft_version":"1.21.1", "loader":null, "start":null
        })).unwrap(), git: None, publication: None }))
    }
    fn candidates() -> Event {
        Event::Candidates { candidates: vec![serde_json::from_value(serde_json::json!({
            "metadata":{"version":1,"minecraft_version":"1.21.1","loader":null,"start":{
                "script":"run.bat", "executable":"java", "arguments":["-jar","server.jar"], "windows":true
            }}, "line":1, "main_class":"-jar"
        })).unwrap()] }
    }

    #[test]
    fn requires_operation_specific_phase_order_and_exactly_one_matching_completion() {
        for (kind, phases, completion) in [
            (Kind::Recognition, vec![Phase::Recognizing], candidates()),
            (
                Kind::Initialization,
                vec![Phase::Validating, Phase::Writing],
                initialized(),
            ),
        ] {
            let mut valid = validator(kind);
            for phase in phases {
                valid.accept(&stage(phase)).unwrap();
            }
            valid.accept(&completion).unwrap();
            assert!(valid.done);
            assert!(valid.accept(&completion).is_err());
            assert!(valid.accept(&stage(Phase::Writing)).is_err());
        }
        for (kind, events) in [
            (Kind::Recognition, vec![candidates()]),
            (Kind::Recognition, vec![stage(Phase::Validating)]),
            (
                Kind::Recognition,
                vec![stage(Phase::Recognizing), initialized()],
            ),
            (
                Kind::Recognition,
                vec![
                    stage(Phase::Recognizing),
                    Event::Candidates {
                        candidates: Vec::new(),
                    },
                ],
            ),
            (Kind::Initialization, vec![stage(Phase::Writing)]),
            (
                Kind::Initialization,
                vec![stage(Phase::Validating), initialized()],
            ),
            (
                Kind::Initialization,
                vec![stage(Phase::Validating), stage(Phase::Validating)],
            ),
            (
                Kind::Initialization,
                vec![
                    stage(Phase::Validating),
                    stage(Phase::Writing),
                    candidates(),
                ],
            ),
        ] {
            let mut current = validator(kind);
            assert!(events.iter().any(|event| current.accept(event).is_err()));
        }
    }
}
