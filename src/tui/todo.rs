use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tools::TodoItem;

use super::theme;

pub fn lines(todos: &[TodoItem]) -> Vec<Line<'static>> {
    todos
        .iter()
        .map(|todo| {
            let (marker, color) = match todo.status.as_str() {
                "completed" => ("x", theme::DIM),
                "in_progress" => ("~", theme::ACCENT),
                _ => (" ", theme::ASSISTANT),
            };
            let mut style = Style::default().fg(color);
            if todo.status == "in_progress" {
                style = style.add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(format!("[{marker}] "), style),
                Span::styled(todo.content.clone(), style),
                Span::styled(format!("  ({})", todo.priority), Style::default().fg(theme::DIM)),
            ])
        })
        .collect()
}
