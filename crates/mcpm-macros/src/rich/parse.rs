//! Pure parser from the `rich!` literal to blocks and role spans; it knows nothing about tokens.

/// 1-based position inside the literal's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pos {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub(crate) pos: Pos,
    pub(crate) message: String,
}

impl ParseError {
    fn new(pos: Pos, message: impl Into<String>) -> Self {
        Self {
            pos,
            message: message.into(),
        }
    }
}

/// Mirrors `mcpm_rich::Role`; names are the markup spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Normal,
    Code,
    Primary,
    Secondary,
    Important,
    Muted,
    Success,
    Warning,
    Danger,
}

impl Role {
    pub(crate) fn variant(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Code => "Code",
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Important => "Important",
            Self::Muted => "Muted",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::Danger => "Danger",
        }
    }

    /// Roles a `[text](role)` marker may name; `Normal` and `Code` have their own syntax.
    fn marker(name: &str) -> Option<Self> {
        Some(match name {
            "primary" => Self::Primary,
            "secondary" => Self::Secondary,
            "important" => Self::Important,
            "muted" => Self::Muted,
            "success" => Self::Success,
            "warning" => Self::Warning,
            "danger" => Self::Danger,
            _ => return None,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Paragraph(Vec<Span>),
    List(Vec<Vec<Span>>),
}

/// A run of literal text and placeholders sharing one role.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) role: Role,
    pub(crate) pieces: Vec<Piece>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Literal(String),
    Placeholder { reference: Reference, spec: String },
}

/// Implicit `{}` placeholders are already numbered in literal order, as `format!` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    Index(usize),
    Name(String),
}

type Cell = (char, Pos);
type Line = Vec<Cell>;

pub(crate) fn parse(source: &str) -> Result<Vec<Block>, ParseError> {
    let lines: Vec<Line> = source
        .split('\n')
        .enumerate()
        .map(|(line, text)| {
            text.trim_end_matches('\r')
                .chars()
                .enumerate()
                .map(|(column, ch)| {
                    (
                        ch,
                        Pos {
                            line: line + 1,
                            column: column + 1,
                        },
                    )
                })
                .collect()
        })
        .collect();
    let (Some(first), Some(last)) = (
        lines.iter().position(|line| !is_blank(line)),
        lines.iter().rposition(|line| !is_blank(line)),
    ) else {
        return Err(ParseError::new(
            Pos { line: 1, column: 1 },
            "the text is empty",
        ));
    };
    let indent = lines[first..=last]
        .iter()
        .filter(|line| !is_blank(line))
        .map(leading_whitespace)
        .min()
        .unwrap_or(0);
    let mut groups: Vec<Vec<Line>> = vec![Vec::new()];
    for line in &lines[first..=last] {
        if is_blank(line) {
            if !groups.last().is_some_and(Vec::is_empty) {
                groups.push(Vec::new());
            }
        } else {
            groups
                .last_mut()
                .expect("groups starts non-empty")
                .push(line[indent..].to_vec());
        }
    }
    groups.retain(|group| !group.is_empty());
    let mut counter = 0;
    groups
        .into_iter()
        .map(|group| block(group, &mut counter))
        .collect()
}

fn block(group: Vec<Line>, counter: &mut usize) -> Result<Block, ParseError> {
    let quoted = group.iter().filter(|line| starts_with(line, "> ")).count();
    let (base, group) = if quoted == group.len() {
        (
            Role::Muted,
            group.into_iter().map(|line| line[2..].to_vec()).collect(),
        )
    } else if quoted == 0 {
        (Role::Normal, group)
    } else {
        let offender = group
            .iter()
            .find(|line| starts_with(line, "> ") != starts_with(&group[0], "> "))
            .expect("a mixed block has an offending line");
        return Err(ParseError::new(
            offender[0].1,
            "a block must either start every line with `> ` or none of them",
        ));
    };
    if !starts_with(&group[0], "- ") {
        if let Some(line) = group[1..].iter().find(|line| starts_with(line, "- ")) {
            return Err(ParseError::new(
                line[0].1,
                "a line starting with `- ` inside a paragraph is ambiguous; separate the list with a blank line",
            ));
        }
        return Ok(Block::Paragraph(inline(&join(group), base, counter)?));
    }
    let mut items: Vec<Vec<Line>> = Vec::new();
    for line in group {
        if starts_with(&line, "- ") {
            items.push(vec![line[2..].to_vec()]);
        } else if leading_whitespace(&line) >= 2 {
            items
                .last_mut()
                .expect("a list starts with an item")
                .push(line);
        } else {
            return Err(ParseError::new(
                line[0].1,
                "a list item continuation must be indented by at least 2 spaces",
            ));
        }
    }
    Ok(Block::List(
        items
            .into_iter()
            .map(|item| inline(&join(item), base, counter))
            .collect::<Result<_, _>>()?,
    ))
}

/// Joins lines with one space; leading and trailing whitespace of each line is dropped.
fn join(lines: Vec<Line>) -> Line {
    let mut joined: Line = Vec::new();
    for line in lines {
        let start = leading_whitespace(&line);
        let end = line
            .iter()
            .rposition(|(ch, _)| !ch.is_whitespace())
            .map_or(start, |index| index + 1);
        let trimmed = &line[start.min(end)..end];
        if let Some(&(_, pos)) = trimmed.first() {
            if !joined.is_empty() {
                joined.push((' ', pos));
            }
            joined.extend_from_slice(trimmed);
        }
    }
    joined
}

/// Splits one paragraph or item into role spans, resolving escapes and placeholders.
///
/// `base` is the role of unmarked text: `Normal`, or `Muted` inside a `> ` block.
fn inline(cells: &[Cell], base: Role, counter: &mut usize) -> Result<Vec<Span>, ParseError> {
    let mut builder = Builder::new(base);
    // Open code span or role marker: position and, for a marker, its `[` start.
    let mut code: Option<Pos> = None;
    let mut marker: Option<Pos> = None;
    let mut index = 0;
    while index < cells.len() {
        let (ch, pos) = cells[index];
        let next = cells.get(index + 1).map(|&(next, _)| next);
        match ch {
            '`' | '{' | '}' | '[' | ']' if next == Some(ch) => {
                builder.literal(ch);
                index += 2;
            }
            '`' if code.is_some() => {
                code = None;
                builder.switch(builder.outer);
                index += 1;
            }
            '`' => {
                if marker.is_some() {
                    return Err(ParseError::new(
                        pos,
                        "code spans cannot appear inside a `[text](role)` marker",
                    ));
                }
                code = Some(pos);
                builder.switch(Role::Code);
                index += 1;
            }
            '[' if code.is_none() => {
                if marker.is_some() {
                    return Err(ParseError::new(pos, "role markers cannot be nested"));
                }
                marker = Some(pos);
                // The role is only known at `](role)`; collect into a pending span.
                builder.switch(Role::Normal);
                index += 1;
            }
            ']' if code.is_none() => {
                let Some(open) = marker.take() else {
                    return Err(ParseError::new(
                        pos,
                        "unmatched `]`; write `]]` for a literal bracket",
                    ));
                };
                if cells.get(index + 1).map(|&(ch, _)| ch) != Some('(') {
                    return Err(ParseError::new(
                        pos,
                        "`[text]` must be followed by `(role)`; write `[[`/`]]` for literal brackets",
                    ));
                }
                let Some(length) = cells[index + 2..].iter().position(|&(ch, _)| ch == ')') else {
                    return Err(ParseError::new(pos, "unclosed `(role)`"));
                };
                let name: String = cells[index + 2..index + 2 + length]
                    .iter()
                    .map(|&(ch, _)| ch)
                    .collect();
                let Some(role) = Role::marker(&name) else {
                    return Err(ParseError::new(
                        cells[index + 1].1,
                        format!(
                            "unknown role `{name}`; expected primary, secondary, important, muted, success, warning or danger"
                        ),
                    ));
                };
                if !builder.close_marker(role) {
                    return Err(ParseError::new(
                        open,
                        "a `[text](role)` marker must not be empty",
                    ));
                }
                index += length + 3;
            }
            '{' => {
                let Some(length) = cells[index + 1..].iter().position(|&(ch, _)| ch == '}') else {
                    return Err(ParseError::new(
                        pos,
                        "unclosed `{`; write `{{` for a literal brace",
                    ));
                };
                let content = &cells[index + 1..index + 1 + length];
                if let Some(&(_, inner)) = content.iter().find(|&&(ch, _)| ch == '{') {
                    return Err(ParseError::new(
                        inner,
                        "unclosed `{`; write `{{` for a literal brace",
                    ));
                }
                let content: String = content.iter().map(|&(ch, _)| ch).collect();
                builder.placeholder(placeholder(&content, pos, counter)?);
                index += length + 2;
            }
            '}' => {
                return Err(ParseError::new(
                    pos,
                    "unmatched `}`; write `}}` for a literal brace",
                ));
            }
            ch => {
                builder.literal(ch);
                index += 1;
            }
        }
    }
    if let Some(pos) = code {
        return Err(ParseError::new(
            pos,
            "unclosed code span; write two backticks for a literal backtick",
        ));
    }
    if let Some(pos) = marker {
        return Err(ParseError::new(
            pos,
            "unclosed `[`; write `[[` for a literal bracket",
        ));
    }
    Ok(builder.finish())
}

fn placeholder(content: &str, pos: Pos, counter: &mut usize) -> Result<Piece, ParseError> {
    let (reference, spec) = content.split_once(':').unwrap_or((content, ""));
    if spec.contains(['$', '*']) {
        return Err(ParseError::new(
            pos,
            "format specs with `$` or `*` are not supported",
        ));
    }
    let reference = if reference.is_empty() {
        *counter += 1;
        Reference::Index(*counter - 1)
    } else if reference.bytes().all(|byte| byte.is_ascii_digit()) {
        Reference::Index(
            reference
                .parse()
                .map_err(|_| ParseError::new(pos, "argument index is too large"))?,
        )
    } else if is_identifier(reference) {
        Reference::Name(reference.into())
    } else {
        return Err(ParseError::new(
            pos,
            format!("`{{{content}}}` is not a valid placeholder; expected an index or identifier"),
        ));
    };
    Ok(Piece::Placeholder {
        reference,
        spec: spec.into(),
    })
}

/// Accumulates pieces and starts a new span whenever the role changes.
struct Builder {
    spans: Vec<Span>,
    pieces: Vec<Piece>,
    text: String,
    role: Role,
    /// Role of unmarked text in this block.
    outer: Role,
}

impl Builder {
    fn new(outer: Role) -> Self {
        Self {
            spans: Vec::new(),
            pieces: Vec::new(),
            text: String::new(),
            role: outer,
            outer,
        }
    }

    fn literal(&mut self, ch: char) {
        self.text.push(ch);
    }

    fn placeholder(&mut self, piece: Piece) {
        self.flush_text();
        self.pieces.push(piece);
    }

    fn switch(&mut self, role: Role) {
        self.flush_span();
        self.role = role;
    }

    /// Assigns the marker's role to the pending pieces; false if the marker was empty.
    fn close_marker(&mut self, role: Role) -> bool {
        self.flush_text();
        if self.pieces.is_empty() {
            return false;
        }
        self.role = role;
        self.switch(self.outer);
        true
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            self.pieces
                .push(Piece::Literal(std::mem::take(&mut self.text)));
        }
    }

    fn flush_span(&mut self) {
        self.flush_text();
        if self.pieces.is_empty() {
            return;
        }
        let pieces = std::mem::take(&mut self.pieces);
        // Adjacent spans of one role merge, so markup boundaries do not leak into the data.
        match self.spans.last_mut() {
            Some(last) if last.role == self.role => {
                for piece in pieces {
                    match (last.pieces.last_mut(), piece) {
                        (Some(Piece::Literal(text)), Piece::Literal(more)) => text.push_str(&more),
                        (_, piece) => last.pieces.push(piece),
                    }
                }
            }
            _ => self.spans.push(Span {
                role: self.role,
                pieces,
            }),
        }
    }

    fn finish(mut self) -> Vec<Span> {
        self.flush_span();
        self.spans
    }
}

fn is_blank(line: &Line) -> bool {
    line.iter().all(|(ch, _)| ch.is_whitespace())
}

fn leading_whitespace(line: &Line) -> usize {
    line.iter().take_while(|(ch, _)| ch.is_whitespace()).count()
}

fn starts_with(line: &Line, prefix: &str) -> bool {
    let mut cells = line.iter();
    prefix
        .chars()
        .all(|expected| cells.next().is_some_and(|&(ch, _)| ch == expected))
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(role: Role, text: &str) -> Span {
        Span {
            role,
            pieces: vec![Piece::Literal(text.into())],
        }
    }

    fn error(source: &str) -> (usize, usize, String) {
        let error = parse(source).unwrap_err();
        (error.pos.line, error.pos.column, error.message)
    }

    #[test]
    fn common_indentation_and_surrounding_blank_lines_are_removed() {
        let blocks = parse("\n    Run `mcpm info`\n      now.\n\n    > Note.\n").unwrap();
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![
                    span(Role::Normal, "Run "),
                    span(Role::Code, "mcpm info"),
                    span(Role::Normal, " now."),
                ]),
                Block::Paragraph(vec![span(Role::Muted, "Note.")]),
            ]
        );
    }

    #[test]
    fn muted_lists_keep_hanging_continuations_inside_their_item() {
        let blocks = parse("> - zsh: set `PATH`.\n> - bash: use\n>   the file.").unwrap();
        assert_eq!(
            blocks,
            [Block::List(vec![
                vec![
                    span(Role::Muted, "zsh: set "),
                    span(Role::Code, "PATH"),
                    span(Role::Muted, "."),
                ],
                vec![span(Role::Muted, "bash: use the file.")],
            ])]
        );
    }

    #[test]
    fn role_markers_return_to_the_block_role_and_merge_equal_neighbours() {
        let blocks =
            parse("[Done](success) [[x]] > [a](muted)[b](muted)\n\n> c [d](danger) e").unwrap();
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![
                    span(Role::Success, "Done"),
                    span(Role::Normal, " [x] > "),
                    span(Role::Muted, "ab"),
                ]),
                Block::Paragraph(vec![
                    span(Role::Muted, "c "),
                    span(Role::Danger, "d"),
                    span(Role::Muted, " e"),
                ]),
            ]
        );
    }

    #[test]
    fn escapes_and_placeholders_resolve_in_every_role() {
        let blocks = parse("`` {{x}} `a``b` `{}` [{name:?}](secondary) {1}").unwrap();
        let [Block::Paragraph(spans)] = blocks.as_slice() else {
            panic!("one paragraph expected")
        };
        let placeholder = |reference, spec: &str| Piece::Placeholder {
            reference,
            spec: spec.into(),
        };
        assert_eq!(
            spans,
            &[
                span(Role::Normal, "` {x} "),
                span(Role::Code, "a`b"),
                span(Role::Normal, " "),
                Span {
                    role: Role::Code,
                    pieces: vec![placeholder(Reference::Index(0), "")],
                },
                span(Role::Normal, " "),
                Span {
                    role: Role::Secondary,
                    pieces: vec![placeholder(Reference::Name("name".into()), "?")],
                },
                Span {
                    role: Role::Normal,
                    pieces: vec![
                        Piece::Literal(" ".into()),
                        placeholder(Reference::Index(1), "")
                    ],
                },
            ]
        );
    }

    #[test]
    fn implicit_placeholders_number_in_literal_order_across_blocks() {
        let mut indexes = Vec::new();
        for block in parse("{}\n\n- {} {2}\n- [{}](primary)").unwrap() {
            let items = match block {
                Block::Paragraph(spans) => vec![spans],
                Block::List(items) => items,
            };
            for span in items.iter().flatten() {
                for piece in &span.pieces {
                    if let Piece::Placeholder {
                        reference: Reference::Index(index),
                        ..
                    } = piece
                    {
                        indexes.push(*index);
                    }
                }
            }
        }
        assert_eq!(indexes, [0, 1, 2, 2]);
    }

    #[test]
    fn errors_report_line_and_column_inside_the_literal() {
        let unclosed = error("a\nb `c");
        assert_eq!((unclosed.0, unclosed.1), (2, 3));
        assert_eq!(error("ok {x").0, 1);
        assert_eq!(
            error("stray }"),
            (1, 7, "unmatched `}`; write `}}` for a literal brace".into())
        );
        assert_eq!(error("  \n\n").2, "the text is empty");
        assert!(error("{:>1$}").2.contains("not supported"));
        assert!(error("{a b}").2.contains("not a valid placeholder"));
    }

    #[test]
    fn role_marker_errors_point_at_the_marker() {
        assert!(error("a ] b").2.contains("unmatched `]`"));
        assert!(error("[a] b").2.contains("must be followed by `(role)`"));
        assert_eq!(error("x [a](code)").1, 6);
        assert!(error("[a](normal)").2.contains("unknown role `normal`"));
        assert!(
            error("[a [b](muted)](muted)")
                .2
                .contains("cannot be nested")
        );
        assert!(error("[`a`](muted)").2.contains("code spans cannot appear"));
        assert!(error("[](muted)").2.contains("must not be empty"));
        assert!(error("[a").2.contains("unclosed `[`"));
    }

    #[test]
    fn block_structure_errors_point_at_the_offending_line() {
        assert_eq!(error("> a\nb").0, 2);
        assert_eq!(error("a\n- b").0, 2);
        assert_eq!(error("- a\nb").0, 2);
    }
}
