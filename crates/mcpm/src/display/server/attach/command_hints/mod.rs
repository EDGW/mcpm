//! Command-specific argument assistance, selected from the syntax actually returned by help.

use std::ops::Range;

mod gamerule;

/// Dispatches recognized command syntax without exposing command details to the hint widget.
pub(super) struct Handler(Kind);

impl Handler {
    pub fn from_help(name: &str, arguments: &str) -> Option<Self> {
        match name {
            "gamerule" => {
                gamerule::Gamerule::parse(arguments).map(|handler| Self(Kind::Gamerule(handler)))
            }
            _ => None,
        }
    }

    pub fn syntax(&self) -> &'static str {
        match &self.0 {
            Kind::Gamerule(_) => "<type> <value>",
        }
    }

    pub fn query<'a, 'input>(&'a self, arguments: &'input str) -> Query<'a, 'input> {
        match &self.0 {
            Kind::Gamerule(handler) => handler.query(arguments),
        }
    }
}

enum Kind {
    Gamerule(gamerule::Gamerule),
}

/// Candidates and replacement coordinates refer only to the supplied argument text.
pub(super) struct Query<'a, 'input> {
    pub candidates: &'a [String],
    pub prefix: &'input str,
    pub replacement: Range<usize>,
    pub diagnostic: Option<Diagnostic>,
    pub validates_value: fn(&str) -> bool,
}

impl<'a, 'input> Query<'a, 'input> {
    pub fn matches(&self) -> impl Iterator<Item = &'a str> + use<'a, 'input> {
        let prefix = self.prefix;
        let matches = self
            .candidates
            .iter()
            .map(String::as_str)
            .filter(move |candidate| candidate.starts_with(prefix));
        matches
            .clone()
            .filter(move |candidate| *candidate == prefix)
            .chain(matches.filter(move |candidate| *candidate != prefix))
    }
}

/// Advisory only: the editor highlights this range; command submission does not consume it.
pub(super) struct Diagnostic {
    pub range: Range<usize>,
    pub message: &'static str,
}
