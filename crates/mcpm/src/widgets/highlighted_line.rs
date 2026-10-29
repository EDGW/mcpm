//! Render parser-owned byte ranges as styled text; later ranges override earlier ones.

use std::ops::Range;

use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub(crate) fn highlighted_line(
    source: &str,
    ranges: impl IntoIterator<Item = (Range<usize>, Style)>,
) -> Line<'_> {
    let ranges: Vec<_> = ranges.into_iter().collect();
    let mut boundaries = vec![0, source.len()];
    for (range, _) in &ranges {
        boundaries.extend([range.start, range.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    Line::from(
        boundaries
            .windows(2)
            .map(|bounds| {
                let start = bounds[0];
                let end = bounds[1];
                let paint = ranges
                    .iter()
                    .rev()
                    .find(|(range, _)| range.contains(&start))
                    .map_or(Style::default(), |(_, paint)| *paint);
                Span::styled(&source[start..end], paint)
            })
            .collect::<Vec<_>>(),
    )
}
