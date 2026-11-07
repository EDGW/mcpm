//! Attach builtin command identities and their shared hint descriptions.

#[derive(Clone, Copy)]
pub(super) enum Builtin {
    Detach,
    Stop,
    Kill,
    Speed,
}

pub(super) struct Definition {
    pub command: Builtin,
    pub name: &'static str,
    pub description: &'static str,
}

pub(super) const DEFINITIONS: &[Definition] = &[
    Definition {
        command: Builtin::Detach,
        name: "@detach",
        description: "detach without stopping Java",
    },
    Definition {
        command: Builtin::Stop,
        name: "@stop",
        description: "send stop; never force-kill",
    },
    Definition {
        command: Builtin::Kill,
        name: "@kill",
        description: "force-kill Java",
    },
    Definition {
        command: Builtin::Speed,
        name: "@speed",
        description: "toggle compact 100ms AVG / full traffic rates",
    },
];

pub(super) fn parse(text: &str) -> Option<Builtin> {
    DEFINITIONS
        .iter()
        .find(|entry| entry.name == text)
        .map(|entry| entry.command)
}
