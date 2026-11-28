//! Attach builtin command identities and their shared hint descriptions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Builtin<'a> {
    Detach,
    Stop,
    Kill,
    Speed,
    Debug(&'a str),
}

pub(super) struct Definition {
    pub name: &'static str,
    pub description: &'static str,
}

pub(super) const DEFINITIONS: &[Definition] = &[
    Definition {
        name: "@detach",
        description: "detach without stopping Java",
    },
    Definition {
        name: "@stop",
        description: "send stop; never force-kill",
    },
    Definition {
        name: "@kill",
        description: "force-kill Java",
    },
    Definition {
        name: "@speed",
        description: "toggle compact 100ms AVG / full traffic rates",
    },
    Definition {
        name: "@debug",
        description: "set a frontend debug variable",
    },
];

pub(super) fn parse(text: &str) -> Option<Builtin<'_>> {
    match text {
        "@detach" => Some(Builtin::Detach),
        "@stop" => Some(Builtin::Stop),
        "@kill" => Some(Builtin::Kill),
        "@speed" => Some(Builtin::Speed),
        "@debug" => Some(Builtin::Debug("")),
        _ => {
            let rest = text.strip_prefix("@debug")?;
            let first = rest.as_bytes().first().copied()?;
            if first == b' ' || first == b'\t' {
                Some(Builtin::Debug(rest.trim()))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_builtins_and_debug_with_arguments() {
        assert_eq!(parse("@detach"), Some(Builtin::Detach));
        assert_eq!(parse("@speed"), Some(Builtin::Speed));
        assert_eq!(parse("@debug"), Some(Builtin::Debug("")));
        assert_eq!(
            parse("@debug display_content_range true"),
            Some(Builtin::Debug("display_content_range true"))
        );
        assert_eq!(parse("@debugging"), None);
        assert_eq!(parse("@detach now"), None);
    }
}
