//! Styled single-line text with match emphasis and width-aware context elision.

use std::ops::Range;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Byte ranges refer to the concatenated span contents, excluding any external labels.
/// Matching flips the original bold weight and removes dimming; colors are supplied
/// by the caller, so this widget has no dependency on search syntax or themes.
#[derive(Debug)]
pub(crate) struct MatchedText<'a> {
    text: Line<'a>,
    matches: Vec<Range<usize>>,
    match_style: Style,
    ellipsis_style: Style,
    overflow: Overflow,
}

impl<'a> MatchedText<'a> {
    pub(crate) fn new(text: impl Into<Line<'a>>) -> Self {
        Self {
            text: text.into(),
            matches: Vec::new(),
            match_style: Style::default(),
            ellipsis_style: Style::default(),
            overflow: Overflow::MatchContext,
        }
    }

    pub(crate) fn matches(mut self, matches: Vec<Range<usize>>) -> Self {
        self.matches = matches;
        self
    }

    pub(crate) fn match_style(mut self, style: Style) -> Self {
        self.match_style = style;
        self
    }

    pub(crate) fn ellipsis_style(mut self, style: Style) -> Self {
        self.ellipsis_style = style;
        self
    }

    pub(crate) fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// The same layout is used by table/list cells and direct Widget rendering.
    pub(crate) fn line(&self, width: usize) -> Line<'static> {
        let text = self.text.to_string();
        let mut spans = self.text.spans.iter();
        let mut span = spans.next();
        let mut span_end = span.map_or(0, |span| span.content.len());
        let glyphs: Vec<_> = text
            .grapheme_indices(true)
            .map(|(start, symbol)| {
                while start >= span_end && span.is_some() {
                    span = spans.next();
                    span_end += span.map_or(0, |span| span.content.len());
                }
                let base = self
                    .text
                    .style
                    .patch(span.map_or(Style::default(), |span| span.style));
                let matched = self.matches.iter().any(|range| {
                    range.start < range.end
                        && range.start < start + symbol.len()
                        && range.end > start
                });
                let style = if matched {
                    let style = base.patch(self.match_style).remove_modifier(Modifier::DIM);
                    if base.add_modifier.contains(Modifier::BOLD) {
                        style.remove_modifier(Modifier::BOLD)
                    } else {
                        style.add_modifier(Modifier::BOLD)
                    }
                } else {
                    base
                };
                Glyph {
                    symbol,
                    style,
                    matched,
                }
            })
            .collect();
        let mut widths = vec![0];
        for glyph in &glyphs {
            widths.push(widths.last().unwrap() + glyph.symbol.width());
        }
        let mut output = Vec::new();
        let mut emit = |range: Range<usize>| {
            for glyph in &glyphs[range] {
                append(&mut output, glyph.symbol, glyph.style);
            }
        };
        if widths[glyphs.len()] <= width {
            emit(0..glyphs.len());
            return Line::from(output);
        }
        let dots = width.min(ELLIPSIS.len());
        let tail_end = fitting_end(&widths, 0, width - dots);
        let hit = glyphs.iter().position(|glyph| glyph.matched).map(|start| {
            let end = glyphs[start..]
                .iter()
                .position(|glyph| !glyph.matched)
                .map_or(glyphs.len(), |offset| start + offset);
            start..end
        });
        let Some(hit) =
            hit.filter(|hit| self.overflow == Overflow::MatchContext && hit.end > tail_end)
        else {
            emit(0..tail_end);
            append(&mut output, &ELLIPSIS[..dots], self.ellipsis_style);
            return Line::from(output);
        };
        let (prefix, window, dots) = context(&widths, hit, width);
        emit(0..prefix);
        if prefix < window.start {
            append(&mut output, &ELLIPSIS[..dots], self.ellipsis_style);
        }
        for glyph in &glyphs[window.clone()] {
            append(&mut output, glyph.symbol, glyph.style);
        }
        if window.end < glyphs.len() {
            append(&mut output, &ELLIPSIS[..dots], self.ellipsis_style);
        }
        Line::from(output)
    }
}

impl Widget for MatchedText<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Paragraph::new(self.line(area.width as usize)).render(area, buffer);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Overflow {
    Ellipsis,
    MatchContext,
}

const ELLIPSIS: &str = "...";

#[derive(Debug)]
struct Glyph<'a> {
    symbol: &'a str,
    style: Style,
    matched: bool,
}

/// Reserve the hit first, then share spare columns between the original prefix
/// and context on either side. No filename rules or field-specific widths apply.
fn context(widths: &[usize], hit: Range<usize>, width: usize) -> (usize, Range<usize>, usize) {
    let count = widths.len() - 1;
    let hit_width = widths[hit.end] - widths[hit.start];
    if hit_width > width {
        let gaps = usize::from(hit.start > 0) + 1;
        let dots = ELLIPSIS.len().min(width / (gaps + 1));
        let end = fitting_end(widths, hit.start, width - gaps * dots);
        return (0, hit.start..end, dots);
    }
    let gaps = usize::from(hit.start > 0) + usize::from(hit.end < count);
    let dots = if gaps == 0 {
        0
    } else {
        ELLIPSIS.len().min((width - hit_width) / gaps)
    };
    let cost = |prefix, start, end| {
        widths[prefix] + widths[end] - widths[start]
            + dots * (usize::from(prefix < start) + usize::from(end < count))
    };
    let (mut prefix, mut start, mut end) = (0, hit.start, hit.end);
    loop {
        let previous = (prefix, start, end);
        if prefix < start && cost(prefix + 1, start, end) <= width {
            prefix += 1;
        }
        if start > prefix && cost(prefix, start - 1, end) <= width {
            start -= 1;
        }
        if end < count && cost(prefix, start, end + 1) <= width {
            end += 1;
        }
        if previous == (prefix, start, end) {
            break;
        }
    }
    (prefix, start..end, dots)
}

fn fitting_end(widths: &[usize], start: usize, budget: usize) -> usize {
    start + widths[start..].partition_point(|&width| width - widths[start] <= budget) - 1
}

fn append(spans: &mut Vec<Span<'static>>, value: &str, style: Style) {
    if value.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(value);
    } else {
        spans.push(Span::styled(value.to_owned(), style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn matches_cross_spans_and_invert_original_weight_without_losing_other_styles() {
        let line = MatchedText::new(Line::from(vec![
            Span::styled("ab", Style::default().fg(Color::Red).italic()),
            Span::styled("cd", Style::default().bold().dim().bg(Color::Blue)),
            Span::raw("ef"),
        ]))
        .matches(std::iter::once(1..4).collect())
        .match_style(Style::default().fg(Color::Yellow))
        .line(20);
        assert_eq!(line.to_string(), "abcdef");
        let highlighted: Vec<_> = line
            .spans
            .iter()
            .filter(|span| span.style.fg == Some(Color::Yellow))
            .collect();
        assert_eq!(highlighted.len(), 2);
        assert_eq!(highlighted[0].content, "b");
        assert!(
            highlighted[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC)
        );
        assert_eq!(highlighted[1].content, "cd");
        assert!(
            !highlighted[1]
                .style
                .add_modifier
                .intersects(Modifier::BOLD | Modifier::DIM)
        );
        assert_eq!(highlighted[1].style.bg, Some(Color::Blue));
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn hidden_match_keeps_prefix_and_nearby_context_and_visible_matches_keep_tail_layout() {
        let text = "Create-an-extremely-long-compat-XXXXAABBCC-other.jar";
        let start = text.find("AABBCC").unwrap();
        let widget = MatchedText::new(text)
            .matches(std::iter::once(start..start + 6).collect())
            .match_style(Style::default().fg(Color::Yellow));
        let line = widget.line(24);
        assert!(line.width() <= 24);
        assert!(line.to_string().starts_with("Cre"));
        assert!(line.to_string().contains("..."));
        assert!(line.to_string().contains("XXXXAABBCC"), "{line}");
        assert_eq!(
            line.spans
                .iter()
                .find(|span| span.style.fg == Some(Color::Yellow))
                .unwrap()
                .content,
            "AABBCC"
        );
        assert_eq!(
            MatchedText::new(text)
                .matches(std::iter::once(0..6).collect())
                .line(12)
                .to_string(),
            "Create-an..."
        );
        assert!(
            !widget
                .overflow(Overflow::Ellipsis)
                .line(24)
                .to_string()
                .contains("AABBCC")
        );
    }

    #[test]
    fn unicode_clusters_remain_whole_even_across_spans_and_match_boundaries() {
        let text = "中e\u{301}👩‍💻文tail";
        let emoji = text.find('👩').unwrap();
        let line = MatchedText::new(Line::from(vec![
            Span::raw("中e"),
            Span::raw("\u{301}👩‍💻文tail"),
        ]))
        .matches(
            std::iter::once(
                emoji + '👩'.len_utf8()..emoji + '👩'.len_utf8() + '\u{200d}'.len_utf8(),
            )
            .collect(),
        )
        .match_style(Style::default().fg(Color::Yellow))
        .line(40);
        assert_eq!(line.to_string(), text);
        assert_eq!(
            line.spans
                .iter()
                .find(|span| span.style.fg == Some(Color::Yellow))
                .unwrap()
                .content,
            "👩‍💻"
        );
        assert_eq!(
            MatchedText::new("e\u{301}suffix").line(4).to_string(),
            "e\u{301}..."
        );
    }

    #[test]
    fn tiny_widths_and_overlong_matches_stay_bounded_and_keep_short_hits_complete() {
        for (text, needle) in [
            ("prefix-long-AABBCC-tail", "AABBCC"),
            ("中文前缀👩‍💻命中后缀", "👩‍💻命中"),
            ("long-prefix-e\u{301}-tail", "e\u{301}"),
            ("prefix-averyverylongmatch-tail", "averyverylongmatch"),
        ] {
            let start = text.find(needle).unwrap();
            let widget = MatchedText::new(text)
                .matches(std::iter::once(start..start + needle.len()).collect());
            for width in 0..=text.width() + 1 {
                let line = widget.line(width);
                assert!(line.width() <= width, "width {width}: {line}");
                if needle.width() <= width {
                    assert!(line.to_string().contains(needle), "width {width}: {line}");
                }
            }
        }
    }
}
