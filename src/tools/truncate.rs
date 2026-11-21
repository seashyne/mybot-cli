/// Head + tail truncation that never splits a UTF-8 character.
pub fn head_tail(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let half = max_bytes / 2;
    let head_end = floor_boundary(text, half);
    let tail_start = ceil_boundary(text, text.len() - half);
    let omitted = tail_start.saturating_sub(head_end);
    let combined = format!(
        "{}\n… [{omitted} bytes omitted] …\n{}",
        &text[..head_end],
        &text[tail_start..]
    );
    (combined, true)
}

/// Cut a line to at most `max_chars` characters.
pub fn truncate_line(line: &str, max_chars: usize) -> (&str, bool) {
    if line.chars().count() <= max_chars {
        return (line, false);
    }
    let mut end = 0;
    for (count, (index, _)) in line.char_indices().enumerate() {
        if count == max_chars {
            end = index;
            break;
        }
    }
    (&line[..end], true)
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_untouched() {
        let (text, truncated) = head_tail("hello", 100);
        assert_eq!(text, "hello");
        assert!(!truncated);
    }

    #[test]
    fn long_text_keeps_both_ends() {
        let text = "a".repeat(1000);
        let (out, truncated) = head_tail(&text, 100);
        assert!(truncated);
        assert!(out.starts_with("aaaa"));
        assert!(out.ends_with("aaaa"));
        assert!(out.contains("bytes omitted"));
    }

    #[test]
    fn multibyte_safe() {
        let text = "é".repeat(500);
        let (out, truncated) = head_tail(&text, 101);
        assert!(truncated);
        assert!(out.contains("omitted"));
    }

    #[test]
    fn line_truncation() {
        let (line, cut) = truncate_line("abcdef", 3);
        assert_eq!(line, "abc");
        assert!(cut);
    }
}
