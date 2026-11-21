use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme;

/// Render a markdown-ish block into styled lines.
///
/// Streaming friendly: it never needs the full document to be valid, so a
/// half-written code fence simply renders as code until it closes.
pub fn render(text: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;

    for raw in text.split('\n') {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }

        if in_code {
            out.push(Line::from(Span::styled(
                format!("  │ {line}"),
                Style::default().fg(theme::CODE),
            )));
            continue;
        }

        if trimmed.is_empty() {
            out.push(Line::from(""));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("• ", Style::default().fg(theme::ACCENT)),
            ];
            spans.extend(inline(rest));
            out.push(Line::from(spans));
            continue;
        }

        if let Some((number, rest)) = ordered_item(trimmed) {
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(format!("{number}. "), Style::default().fg(theme::ACCENT)),
            ];
            spans.extend(inline(rest));
            out.push(Line::from(spans));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('>') {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(theme::DIM))];
            spans.extend(inline(rest.trim_start()));
            out.push(Line::from(spans));
            continue;
        }

        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(theme::DIM),
            )));
            continue;
        }

        if trimmed.starts_with("+++") || trimmed.starts_with("---") {
            out.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme::DIM),
            )));
            continue;
        }
        if trimmed.starts_with('+') && !trimmed.starts_with("++") {
            out.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Green),
            )));
            continue;
        }
        if trimmed.starts_with('-') && trimmed.len() > 1 && !trimmed.starts_with("--") {
            out.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Red),
            )));
            continue;
        }

        out.push(Line::from(inline(line)));
    }

    out
}

fn ordered_item(line: &str) -> Option<(u32, &str)> {
    let dot = line.find(". ")?;
    let number = line[..dot].parse::<u32>().ok()?;
    Some((number, &line[dot + 2..]))
}

fn inline(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buffer = String::new();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '`'
            && let Some(end) = find_char(&chars, index + 1, '`') {
                push_buffer(&mut spans, &mut buffer);
                let code: String = chars[index + 1..end].iter().collect();
                spans.push(Span::styled(code, Style::default().fg(theme::CODE)));
                index = end + 1;
                continue;
            }

        if chars[index] == '*' && index + 1 < chars.len() && chars[index + 1] == '*'
            && let Some(end) = find_pair(&chars, index + 2, '*', '*') {
                push_buffer(&mut spans, &mut buffer);
                let bold: String = chars[index + 2..end].iter().collect();
                spans.push(Span::styled(
                    bold,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                index = end + 2;
                continue;
            }

        if chars[index] == '['
            && let Some(close) = find_char(&chars, index + 1, ']')
                && close + 1 < chars.len() && chars[close + 1] == '('
                    && let Some(end) = find_char(&chars, close + 2, ')') {
                        push_buffer(&mut spans, &mut buffer);
                        let label: String = chars[index + 1..close].iter().collect();
                        spans.push(Span::styled(label, Style::default().fg(theme::LINK)));
                        index = end + 1;
                        continue;
                    }

        buffer.push(chars[index]);
        index += 1;
    }

    push_buffer(&mut spans, &mut buffer);
    if spans.is_empty() {
        spans.push(Span::raw(""));
    }
    spans
}

fn push_buffer(spans: &mut Vec<Span<'static>>, buffer: &mut String) {
    if !buffer.is_empty() {
        spans.push(Span::raw(std::mem::take(buffer)));
    }
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == needle)
}

fn find_pair(chars: &[char], from: usize, first: char, second: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&i| chars[i] == first && chars[i + 1] == second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_headings_and_bullets() {
        let lines = render("# Title\n\n- one\n- two\n");
        let text = plain(&lines);
        assert!(text.contains("Title"));
        assert!(text.contains("• one"));
    }

    #[test]
    fn code_fence_keeps_content() {
        let lines = render("before\n```rust\nlet x = 1;\n```\nafter");
        let text = plain(&lines);
        assert!(text.contains("let x = 1;"));
        assert!(!text.contains("```"));
    }

    #[test]
    fn unterminated_fence_is_tolerated() {
        let lines = render("```\npartial");
        let text = plain(&lines);
        assert!(text.contains("partial"));
    }

    #[test]
    fn inline_code_and_bold() {
        let lines = render("use `read` and **bold**");
        let text = plain(&lines);
        assert_eq!(text, "use read and bold");
    }
}
