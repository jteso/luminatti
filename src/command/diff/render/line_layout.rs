//! Text-to-row layout primitives shared by the diff renderer.
//!
//! Keeping wrapping, tab expansion, and background padding here keeps the
//! main renderer focused on deciding *what* a diff row represents.

use ratatui::prelude::*;

/// Generates a diagonal stripe pattern for empty placeholder lines.
pub(super) fn stripe_pattern(width: usize) -> String {
    "╱".repeat(width)
}

/// Extend a line's background to the requested display width.
pub(super) fn pad_background<'a>(
    mut spans: Vec<Span<'a>>,
    target_width: usize,
    background: Color,
) -> Vec<Span<'a>> {
    let current_width: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    if current_width < target_width {
        spans.push(Span::styled(
            " ".repeat(target_width - current_width),
            Style::default().bg(background),
        ));
    }
    spans
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn styled_chars(spans: Vec<Span<'_>>) -> Vec<(char, Style)> {
    spans
        .into_iter()
        .flat_map(|span| {
            let style = span.style;
            span.content
                .to_string()
                .chars()
                .map(move |character| (character, style))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn chars_to_spans(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = chars.iter();
    let Some(&(first_character, first_style)) = chars.next() else {
        return spans;
    };

    let mut style = first_style;
    let mut text = String::from(first_character);
    for &(character, next_style) in chars {
        if next_style == style {
            text.push(character);
        } else {
            spans.push(Span::styled(std::mem::take(&mut text), style));
            text.push(character);
            style = next_style;
        }
    }
    spans.push(Span::styled(text, style));
    spans
}

fn wrap_break(chars: &[(char, Style)], max_width: usize) -> (usize, usize) {
    if chars.len() <= max_width {
        return (chars.len(), chars.len());
    }

    let break_at = (1..=max_width.min(chars.len().saturating_sub(1)))
        .filter(|&index| chars[index].0.is_whitespace())
        .last()
        .unwrap_or(max_width);

    if chars
        .get(break_at)
        .is_some_and(|(character, _)| character.is_whitespace())
    {
        let next_start = chars[break_at..]
            .iter()
            .position(|(character, _)| !character.is_whitespace())
            .map_or(chars.len(), |offset| break_at + offset);
        (break_at, next_start)
    } else {
        (max_width, max_width)
    }
}

/// Wrap a diff row while aligning continuation rows under the original text.
pub(super) fn wrapped_lines(
    prefix_spans: Vec<Span<'_>>,
    content_spans: Vec<Span<'_>>,
    target_width: usize,
    background: Color,
    wrap: bool,
) -> Vec<Line<'static>> {
    if !wrap {
        let spans = prefix_spans
            .into_iter()
            .chain(content_spans)
            .map(|span| Span::styled(span.content.to_string(), span.style))
            .collect();
        return vec![Line::from(pad_background(spans, target_width, background))];
    }

    let prefix_width = span_width(&prefix_spans);
    let content_width = target_width
        .saturating_sub(prefix_width)
        .saturating_sub(1)
        .max(1);
    let chars = styled_chars(content_spans);
    let prefix = prefix_spans
        .iter()
        .map(|span| Span::styled(span.content.to_string(), span.style))
        .collect::<Vec<_>>();
    let blank_prefix = prefix_spans
        .iter()
        .filter_map(|span| {
            let width = span.content.chars().count();
            (width > 0).then(|| Span::styled(" ".repeat(width), span.style))
        })
        .collect::<Vec<_>>();

    if chars.is_empty() {
        return vec![Line::from(pad_background(prefix, target_width, background))];
    }

    let indent = chars
        .iter()
        .take_while(|(character, _)| *character == ' ')
        .count();
    let indent = indent.min(content_width.saturating_sub(1));
    let indent_span =
        (indent > 0).then(|| Span::styled(" ".repeat(indent), Style::default().bg(background)));

    let mut lines = Vec::new();
    let mut start = 0;
    let mut first = true;
    while start < chars.len() {
        let available = if first {
            content_width
        } else {
            content_width.saturating_sub(indent).max(1)
        };
        let remaining = &chars[start..];
        let (take, next_start) = wrap_break(remaining, available);
        let mut spans = if first {
            prefix.clone()
        } else {
            blank_prefix.clone()
        };
        if !first {
            if let Some(indent_span) = &indent_span {
                spans.push(indent_span.clone());
            }
        }
        spans.extend(chars_to_spans(&remaining[..take]));
        lines.push(Line::from(pad_background(spans, target_width, background)));

        if next_start == 0 {
            break;
        }
        start += next_start;
        first = false;
    }
    lines
}

pub(super) fn push_wrapped_line(
    lines: &mut Vec<Line<'static>>,
    prefix_spans: Vec<Span<'_>>,
    content_spans: Vec<Span<'_>>,
    target_width: usize,
    background: Color,
    wrap: bool,
) {
    lines.extend(wrapped_lines(
        prefix_spans,
        content_spans,
        target_width,
        background,
        wrap,
    ));
}

pub(super) fn blank_line(width: usize, background: Color) -> Line<'static> {
    Line::from(vec![Span::styled(
        " ".repeat(width),
        Style::default().bg(background),
    )])
}

pub(super) fn expand_tabs<'a>(spans: Vec<Span<'a>>, tab_width: usize) -> Vec<Span<'a>> {
    let mut column = 0;
    spans
        .into_iter()
        .map(|span| {
            if !span.content.contains('\t') {
                column += span.content.chars().count();
                return span;
            }

            let mut text = String::new();
            for character in span.content.chars() {
                if character == '\t' {
                    let spaces = if tab_width == 0 {
                        0
                    } else {
                        tab_width - (column % tab_width)
                    };
                    text.push_str(&" ".repeat(spaces));
                    column += spaces;
                } else {
                    text.push(character);
                    column += 1;
                }
            }
            Span::styled(text, span.style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: Vec<Line<'static>>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.to_string())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn wraps_at_whitespace_and_aligns_continuation_under_content() {
        let lines = wrapped_lines(
            vec![Span::raw("  1 ")],
            vec![Span::raw("hello world")],
            10,
            Color::Black,
            true,
        );
        assert_eq!(plain(lines), ["  1 hello ", "    world "]);
    }

    #[test]
    fn tab_expansion_tracks_columns_across_style_spans() {
        let spans = expand_tabs(vec![Span::raw("a"), Span::raw("\tb")], 4);
        assert_eq!(spans[1].content, "   b");
    }

    #[test]
    fn zero_width_tabs_are_removed_without_panicking() {
        let spans = expand_tabs(vec![Span::raw("a\tb")], 0);
        assert_eq!(spans[0].content, "ab");
    }
}
