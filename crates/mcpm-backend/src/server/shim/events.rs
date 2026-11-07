//! Bounded live command results; subscriber cursors report loss without blocking execution.

use crate::operations::server::shim::CommandEvent;
use std::{collections::VecDeque, sync::Arc};

const MAX_BYTES: usize = 8 * 1024 * 1024 + 64 * 1024;
const MAX_ITEMS: usize = 128;

#[derive(Default)]
pub(super) struct Events {
    items: VecDeque<(Arc<CommandEvent>, usize)>,
    bytes: usize,
    pub next: u64,
    pub closed: bool,
}
impl Events {
    pub fn push(&mut self, event: CommandEvent) {
        let bytes = match &event {
            CommandEvent::Completed {
                listener_id,
                command_id,
                text,
                result,
            } => {
                listener_id.len()
                    + command_id.len()
                    + text.len()
                    + result.output.as_ref().map_or(0, String::len)
                    + result.diagnostic.as_ref().map_or(0, String::len)
            }
            _ => 0,
        };
        self.items.push_back((Arc::new(event), bytes));
        self.next += 1;
        self.bytes += bytes;
        while self.items.len() > MAX_ITEMS || self.bytes > MAX_BYTES && self.items.len() > 1 {
            self.bytes -= self.items.pop_front().expect("nonempty event buffer").1;
        }
    }

    pub fn read(&self, cursor: &mut u64) -> Option<Arc<CommandEvent>> {
        let oldest = self.next - self.items.len() as u64;
        if *cursor < oldest {
            let missed = oldest - *cursor;
            *cursor = oldest;
            return Some(Arc::new(CommandEvent::Gap { missed }));
        }
        if let Some((event, _)) = self.items.get((*cursor - oldest) as usize) {
            *cursor += 1;
            return Some(event.clone());
        }
        self.closed.then(|| Arc::new(CommandEvent::Done))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::server::shim::{Channel, CommandOutcome, CommandResult, CommandStatus};

    fn completed(text: String) -> CommandEvent {
        CommandEvent::Completed {
            listener_id: "listener".into(),
            command_id: text.clone(),
            text,
            result: CommandResult {
                status: CommandStatus::Accepted,
                channel: Channel::Stdin,
                output: None,
                result: CommandOutcome::Sent,
                diagnostic: None,
            },
        }
    }

    #[test]
    fn lag_reports_gap_then_retained_results_before_done() {
        let mut events = Events::default();
        let mut live = events.next;
        for index in 0..MAX_ITEMS + 3 {
            events.push(completed(index.to_string()));
        }
        events.closed = true;
        assert!(matches!(
            events.read(&mut live).as_deref(),
            Some(CommandEvent::Gap { missed: 3 })
        ));
        for index in 3..MAX_ITEMS + 3 {
            let event = events.read(&mut live).unwrap();
            let CommandEvent::Completed { text, .. } = &*event else {
                panic!("expected result")
            };
            assert_eq!(text, &index.to_string());
        }
        assert!(matches!(
            events.read(&mut live).as_deref(),
            Some(CommandEvent::Done)
        ));
        let mut joined_late = events.next;
        assert!(matches!(
            events.read(&mut joined_late).as_deref(),
            Some(CommandEvent::Done)
        ));
    }
}
