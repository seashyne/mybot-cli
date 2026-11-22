use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}

fn assets() -> &'static Assets {
    static ASSETS: OnceLock<Assets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let mut themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .remove("base16-ocean.dark")
            .or_else(|| themes.themes.into_values().next())
            .unwrap_or_default();
        Assets { syntaxes, theme }
    })
}

fn syntax_for<'a>(assets: &'a Assets, lang: &str) -> Option<&'a SyntaxReference> {
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
    assets
        .syntaxes
        .find_syntax_by_token(lang)
        .or_else(|| assets.syntaxes.find_syntax_by_extension(lang))
        .or_else(|| {
            let lower = lang.to_ascii_lowercase();
            assets
                .syntaxes
                .syntaxes()
                .iter()
                .find(|syntax| syntax.name.to_ascii_lowercase() == lower)
        })
}

/// Highlight one fenced block. Returns `None` when the language is unknown, so
/// the caller can fall back to plain code styling.
pub fn highlight_block(lang: &str, lines: &[String]) -> Option<Vec<Vec<Span<'static>>>> {
    let assets = assets();
    let syntax = syntax_for(assets, lang)?;
    let mut highlighter = syntect::easy::HighlightLines::new(syntax, &assets.theme);

    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let source = format!("{line}\n");
        let regions = highlighter.highlight_line(&source, &assets.syntaxes).ok()?;
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (style, text) in regions {
            let text = text.strip_suffix('\n').unwrap_or(text);
            if text.is_empty() {
                continue;
            }
            spans.push(Span::styled(text.to_string(), convert(style)));
        }
        if spans.is_empty() {
            spans.push(Span::raw(""));
        }
        out.push(spans);
    }
    Some(out)
}

fn convert(style: syntect::highlighting::Style) -> Style {
    let color = style.foreground;
    let mut converted = Style::default().fg(Color::Rgb(color.r, color.g, color.b));
    if style.font_style.contains(FontStyle::BOLD) {
        converted = converted.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        converted = converted.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        converted = converted.add_modifier(Modifier::UNDERLINED);
    }
    converted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust_into_multiple_colored_spans() {
        let lines = vec!["fn main() {".to_string(), "    let x = 1;".to_string()];
        let block = highlight_block("rust", &lines).expect("rust is bundled");
        assert_eq!(block.len(), 2);
        let colors: Vec<Color> = block[0].iter().map(|span| span.style.fg.unwrap()).collect();
        assert!(
            colors.windows(2).any(|pair| pair[0] != pair[1]),
            "expected more than one color, got {colors:?}"
        );
    }

    #[test]
    fn keeps_the_source_text_intact() {
        let lines = vec!["let answer = 42;".to_string()];
        let block = highlight_block("rs", &lines).unwrap();
        let text: String = block[0]
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "let answer = 42;");
    }

    #[test]
    fn unknown_language_returns_none() {
        assert!(highlight_block("not-a-language", &["x".into()]).is_none());
    }
}
