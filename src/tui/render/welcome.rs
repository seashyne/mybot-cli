use std::path::Path;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::{EffortLevel, PermissionMode};
use crate::tui::app::App;
use crate::tui::theme;

const LOGO: [&str; 6] = [
    "███╗   ███╗██╗   ██╗██████╗  ██████╗ ████████╗",
    "████╗ ████║╚██╗ ██╔╝██╔══██╗██╔═══██╗╚══██╔══╝",
    "██╔████╔██║ ╚████╔╝ ██████╔╝██║   ██║   ██║   ",
    "██║╚██╔╝██║  ╚██╔╝  ██╔══██╗██║   ██║   ██║   ",
    "██║ ╚═╝ ██║   ██║   ██████╔╝╚██████╔╝   ██║   ",
    "╚═╝     ╚═╝   ╚═╝   ╚═════╝  ╚═════╝    ╚═╝   ",
];

const LOGO_WIDTH: usize = 46;

pub(super) fn lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let logo_pad = if width >= LOGO_WIDTH + 4 {
        " ".repeat((width - LOGO_WIDTH) / 2)
    } else {
        " ".to_string()
    };

    for (index, row) in LOGO.iter().enumerate() {
        out.push(Line::from(Span::styled(
            format!("{logo_pad}{row}"),
            Style::default()
                .fg(theme::GRADIENT[index.min(theme::GRADIENT.len() - 1)])
                .add_modifier(Modifier::BOLD),
        )));
    }
    out.push(Line::from(""));

    if width < 54 {
        append_compact(&mut out, app, width);
        return out;
    }

    let card_w = width.saturating_sub(2).max(54);
    let inner_w = card_w - 2;
    let margin = " ".repeat((width - card_w) / 2);

    out.push(top_border(&margin, inner_w, app.busy));
    out.push(card_row(
        &margin,
        inner_w,
        vec![
            Span::raw("  "),
            Span::styled(
                "Tips for getting started",
                Style::default().fg(theme::USER).add_modifier(Modifier::BOLD),
            ),
        ],
    ));
    for tip in [
        "Say what you want and how to verify it.",
        "Use /permissions for tool approvals, or /init for AGENTS.md.",
        "Use /model to switch models, /key to set API keys, /help for all.",
    ] {
        out.push(tip_row(&margin, inner_w, tip, false));
    }

    out.push(Line::from(Span::styled(
        format!("{margin}├{}┤", "─".repeat(inner_w)),
        Style::default().fg(theme::BORDER),
    )));
    out.push(card_row(
        &margin,
        inner_w,
        vec![
            Span::raw("  "),
            Span::styled(
                "What's new",
                Style::default().fg(theme::USER).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(theme::DIM)),
            Span::styled(
                "/changelog",
                Style::default()
                    .fg(theme::ASSISTANT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" for history", Style::default().fg(theme::DIM)),
        ],
    ));
    for item in crate::version::latest_changelog_highlights(3) {
        out.push(tip_row(&margin, inner_w, &item, true));
    }
    out.push(Line::from(Span::styled(
        format!("{margin}╰{}╯", "─".repeat(inner_w)),
        Style::default().fg(theme::BORDER),
    )));
    out
}

fn append_compact(out: &mut Vec<Line<'static>>, app: &App, width: usize) {
    let effort = EffortLevel::from_budget(app.status.thinking_budget);
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("mybot cli {}", crate::version::VERSION),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ready", Style::default().fg(theme::CODE)),
    ]));
    out.push(Line::from(Span::styled(
        truncate(
            &format!(
                "  {}/{} · {} · effort {}",
                app.status.provider,
                app.status.model,
                permission_label(app.status.permission),
                effort.label()
            ),
            width,
        ),
        Style::default().fg(theme::DIM),
    )));
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "  Type a request, or /help for commands.",
        Style::default().fg(theme::DIM),
    )));
    out.push(Line::from(Span::styled(
        "  /model · /effort · /changelog · Esc cancels",
        Style::default().fg(theme::DIM),
    )));
}

fn top_border(margin: &str, inner_w: usize, busy: bool) -> Line<'static> {
    let left_prefix = "─ ";
    let title = format!("mybot cli {}", crate::version::VERSION);
    let status_text = if busy { " Working " } else { " Ready " };
    let right_tail = "─";
    let used = left_prefix.chars().count()
        + title.chars().count()
        + 2 // spaces flanking the fill line
        + 1 // status dot
        + status_text.chars().count()
        + right_tail.chars().count();
    let fill = inner_w.saturating_sub(used);
    let dot_color = if busy { theme::TOOL } else { theme::USER };

    Line::from(vec![
        Span::styled(format!("{margin}╭{left_prefix}"), Style::default().fg(theme::BORDER)),
        Span::styled(
            title,
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", "─".repeat(fill)),
            Style::default().fg(theme::BORDER),
        ),
        Span::styled("●", Style::default().fg(dot_color)),
        Span::styled(status_text, Style::default().fg(theme::ASSISTANT)),
        Span::styled(format!("{right_tail}╮"), Style::default().fg(theme::BORDER)),
    ])
}

fn tip_row(margin: &str, inner_w: usize, text: &str, is_changelog: bool) -> Line<'static> {
    let available = inner_w.saturating_sub(6);
    let clipped = truncate(text, available);
    let mut spans = vec![
        Span::raw("  "),
        Span::styled("› ", Style::default().fg(theme::ACCENT)),
    ];
    let dim = Style::default().fg(theme::DIM);
    let bright = Style::default().fg(theme::ASSISTANT);

    if is_changelog && let Some((head, tail)) = clipped.split_once(" — ") {
        spans.extend(highlight_inline(head, bright));
        spans.push(Span::styled(" — ", dim));
        spans.extend(highlight_inline(tail, dim));
    } else {
        spans.extend(highlight_inline(&clipped, dim));
    }

    card_row(margin, inner_w, spans)
}

fn highlight_inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let hi = Style::default()
        .fg(theme::ASSISTANT)
        .add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut plain_start = 0usize;
    let mut i = 0usize;

    while i < text.len() {
        let rest = &text[i..];
        let prev = text[..i].chars().next_back();
        let at_word_start = matches!(prev, None | Some(' ' | '('));
        let token_len = if at_word_start && rest.starts_with('/') {
            let alpha = rest[1..]
                .bytes()
                .take_while(|b| b.is_ascii_lowercase())
                .count();
            if alpha > 0 { Some(1 + alpha) } else { None }
        } else {
            ["Ctrl+V", "Ctrl+O", "Alt+V", "AGENTS.md"]
                .into_iter()
                .find(|kw| at_word_start && rest.starts_with(kw))
                .map(str::len)
        };

        if let Some(len) = token_len {
            if plain_start < i {
                spans.push(Span::styled(text[plain_start..i].to_string(), base));
            }
            spans.push(Span::styled(text[i..i + len].to_string(), hi));
            i += len;
            plain_start = i;
        } else if let Some(ch) = rest.chars().next() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    if plain_start < text.len() {
        spans.push(Span::styled(text[plain_start..].to_string(), base));
    }
    spans
}

fn card_row(margin: &str, inner_w: usize, mut spans: Vec<Span<'static>>) -> Line<'static> {
    fit_spans(&mut spans, inner_w);
    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let pad = inner_w.saturating_sub(used);

    let mut row = Vec::with_capacity(spans.len() + 3);
    row.push(Span::styled(
        format!("{margin}│"),
        Style::default().fg(theme::BORDER),
    ));
    row.extend(spans);
    if pad > 0 {
        row.push(Span::raw(" ".repeat(pad)));
    }
    row.push(Span::styled("│", Style::default().fg(theme::BORDER)));
    Line::from(row)
}

fn fit_spans(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut used = 0usize;
    let mut kept = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        let len = span.content.chars().count();
        if used + len <= max_width {
            used += len;
            kept.push(span);
        } else {
            let remaining = max_width.saturating_sub(used);
            if remaining > 0 {
                kept.push(Span::styled(
                    truncate(&span.content, remaining),
                    span.style,
                ));
            }
            break;
        }
    }
    *spans = kept;
}

pub(in crate::tui::render) fn permission_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "ask first",
        PermissionMode::Auto => "auto approve",
        PermissionMode::Full => "full access",
    }
}

pub(in crate::tui::render) fn permission_style(mode: PermissionMode) -> Style {
    match mode {
        PermissionMode::Ask => Style::default().fg(theme::TOOL),
        PermissionMode::Auto => Style::default().fg(theme::CODE),
        PermissionMode::Full => Style::default()
            .fg(theme::ERROR)
            .add_modifier(Modifier::BOLD),
    }
}

pub(in crate::tui::render) fn short_cwd(cwd: &Path, max_chars: usize) -> String {
    let text = match dirs::home_dir() {
        Some(home) if cwd == home => "~".to_string(),
        Some(home) => match cwd.strip_prefix(&home) {
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => cwd.display().to_string(),
        },
        None => cwd.display().to_string(),
    };
    let count = text.chars().count();
    if count <= max_chars || max_chars < 4 {
        return text;
    }
    let tail: String = text
        .chars()
        .skip(count - (max_chars - 1))
        .collect();
    format!("…{tail}")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
