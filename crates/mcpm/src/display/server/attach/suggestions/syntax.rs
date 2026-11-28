//! Command syntax abstraction for attach suggestions: plain hints, checked argument parsers,
//! and server-state-aware command recommendation validity.

use mcpm_backend::operations::server::shim::ServerStatus;
use std::{borrow::Cow, ops::Range};

const BOOLEAN_VALUES: &[&str] = &["true", "false"];

/// Whether submitting a command is currently recommended, with an explanation when it may be ineffective.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) enum Recommendation {
    Valid,
    Invalid { reason: Cow<'static, str> },
}

/// Policy determining when a command suggestion is recommended for submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum ValidityPolicy {
    /// Always recommended regardless of Minecraft server status.
    Always,
    /// Only recommended when `ServerStatus == Done` (all Minecraft commands and `@stop`).
    RequiresServerDone,
}

impl ValidityPolicy {
    pub fn evaluate(self, status: ServerStatus) -> Recommendation {
        match self {
            Self::Always => Recommendation::Valid,
            Self::RequiresServerDone => match status {
                ServerStatus::Done => Recommendation::Valid,
                ServerStatus::StartingOrUnknown => Recommendation::Invalid {
                    reason: Cow::Borrowed("Server is still starting"),
                },
                ServerStatus::GeneratingChunks(progress) => Recommendation::Invalid {
                    reason: Cow::Owned(format!("Server is generating chunks ({progress}%)")),
                },
                ServerStatus::Stopping => Recommendation::Invalid {
                    reason: Cow::Borrowed("Server is stopping"),
                },
                ServerStatus::Exited => Recommendation::Invalid {
                    reason: Cow::Borrowed("Server has exited"),
                },
            },
        }
    }
}

/// Syntax capability attached to a single command suggestion.
pub(in super::super) struct Syntax(SyntaxKind);

enum SyntaxKind {
    /// Simple hint without validation or argument completion; empty text means no arguments.
    Plain(Cow<'static, str>),
    /// Structured syntax hint with argument completion and live advisory diagnostics.
    Checked(Box<dyn SyntaxChecker>),
}

impl Syntax {
    pub fn plain(text: impl Into<Cow<'static, str>>) -> Self {
        Self(SyntaxKind::Plain(text.into()))
    }

    pub(super) fn checked(checker: impl SyntaxChecker + 'static) -> Self {
        Self(SyntaxKind::Checked(Box::new(checker)))
    }

    pub(super) fn summary(&self) -> &str {
        match &self.0 {
            SyntaxKind::Plain(text) => text.as_ref(),
            SyntaxKind::Checked(checker) => checker.summary(),
        }
    }

    pub(super) fn checker(&self) -> Option<&dyn SyntaxChecker> {
        match &self.0 {
            SyntaxKind::Plain(_) => None,
            SyntaxKind::Checked(checker) => Some(checker.as_ref()),
        }
    }

    pub(super) fn has_arguments(&self) -> bool {
        match &self.0 {
            SyntaxKind::Plain(text) => !text.is_empty(),
            SyntaxKind::Checked(_) => true,
        }
    }
}

/// Argument-level completion and advisory validation for one command.
///
/// All offsets in returned [`SyntaxQuery`] values are relative to the supplied `arguments` slice.
pub(super) trait SyntaxChecker: Send + Sync {
    /// Short syntax summary displayed in command lists and the argument panel header.
    fn summary(&self) -> &str;

    /// Evaluates the argument text (after the command name and leading whitespace).
    fn query<'a, 'input>(&'a self, arguments: &'input str) -> SyntaxQuery<'a, 'input>;
}

/// Visual emphasis for an argument candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CandidateEmphasis {
    Primary,
    Normal,
}

/// One argument completion candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Candidate<'a> {
    pub text: &'a str,
    pub description: Option<&'a str>,
    pub emphasis: CandidateEmphasis,
    /// Whether accepting this candidate at end-of-line appends a trailing space for the next argument.
    pub append_space: bool,
}

/// Result of evaluating a command's argument text against its [`SyntaxChecker`].
pub(super) struct SyntaxQuery<'a, 'input> {
    pub candidates: Vec<Candidate<'a>>,
    pub prefix: &'input str,
    pub replacement: Range<usize>,
    pub diagnostic: Option<Diagnostic>,
}

impl<'a, 'input> SyntaxQuery<'a, 'input> {
    pub fn matches(&self) -> impl Iterator<Item = Candidate<'a>> + '_ {
        let prefix = self.prefix;
        let matched = self
            .candidates
            .iter()
            .copied()
            .filter(move |candidate| candidate.text.starts_with(prefix));
        matched
            .clone()
            .filter(move |candidate| candidate.text == prefix)
            .chain(matched.filter(move |candidate| candidate.text != prefix))
    }
}

/// Advisory diagnostic: highlights a range in the input box without blocking submission.
#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Diagnostic {
    pub range: Range<usize>,
    pub message: Cow<'static, str>,
}

/// Whitespace-delimited token slice and its byte range within an argument string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token<'a> {
    pub text: &'a str,
    pub range: Range<usize>,
    /// True when trailing whitespace follows this token in the source argument string.
    pub complete: bool,
}

/// Returns the next whitespace-delimited token at or after `from` in `arguments`.
pub(super) fn next_token(arguments: &str, from: usize) -> Option<Token<'_>> {
    let rest = arguments.get(from..)?;
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let start = arguments.len() - trimmed.len();
    let len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let end = start + len;
    Some(Token {
        text: &arguments[start..end],
        range: start..end,
        complete: end < arguments.len(),
    })
}

/// Returns a byte range covering all remaining non-trailing-whitespace text starting at `from`.
pub(super) fn trailing_range(arguments: &str, from: usize) -> Range<usize> {
    let end = arguments.trim_end().len().max(from);
    from..end
}

/// Evaluates a terminal `true | false` argument positioned at or after `from` in `arguments`.
///
/// Legal prefixes (`t`, `tr`, `f`, ...) stay diagnostic-free while typing (no trailing whitespace)
/// and offer matching candidates; invalid text or trailing extra tokens produce advisory diagnostics.
pub(super) fn boolean_argument_query<'input>(
    arguments: &'input str,
    from: usize,
) -> SyntaxQuery<'static, 'input> {
    let bool_candidates = || {
        BOOLEAN_VALUES
            .iter()
            .map(|&value| Candidate {
                text: value,
                description: None,
                emphasis: CandidateEmphasis::Primary,
                append_space: false,
            })
            .collect()
    };

    let Some(token) = next_token(arguments, from) else {
        let end = arguments.len();
        return SyntaxQuery {
            candidates: bool_candidates(),
            prefix: "",
            replacement: end..end,
            diagnostic: None,
        };
    };

    let exact = matches!(token.text, "true" | "false");
    let extra = next_token(arguments, token.range.end);

    if !exact {
        let prefix_valid = BOOLEAN_VALUES
            .iter()
            .any(|candidate| candidate.starts_with(token.text));
        let diagnostic = if prefix_valid && !token.complete && extra.is_none() {
            None
        } else {
            Some(Diagnostic {
                range: token.range.clone(),
                message: Cow::Borrowed("Expected true or false"),
            })
        };
        let candidates = if token.complete || extra.is_some() {
            Vec::new()
        } else {
            bool_candidates()
        };
        return SyntaxQuery {
            candidates,
            prefix: token.text,
            replacement: token.range,
            diagnostic,
        };
    }

    if let Some(extra_token) = extra {
        return SyntaxQuery {
            candidates: Vec::new(),
            prefix: "",
            replacement: extra_token.range.clone(),
            diagnostic: Some(Diagnostic {
                range: trailing_range(arguments, extra_token.range.start),
                message: Cow::Borrowed("Unexpected extra argument"),
            }),
        };
    }

    SyntaxQuery {
        candidates: if token.complete {
            Vec::new()
        } else {
            bool_candidates()
        },
        prefix: token.text,
        replacement: token.range,
        diagnostic: None,
    }
}
