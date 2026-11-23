//! Exact and whitespace-tolerant text replacement for the `edit` tool.

#[derive(Debug, Clone)]
pub struct Edit {
    pub old: String,
    pub new: String,
    pub replace_all: bool,
}

pub fn apply_edits(original: &str, edits: &[Edit]) -> Result<String, String> {
    let mut content = original.to_string();

    for (index, raw_edit) in edits.iter().enumerate() {
        let position = index + 1;
        let edit = clean_line_numbers(raw_edit);
        if edit.old.is_empty() {
            return Err(format!("edit #{position}: oldText must not be empty"));
        }

        if edit.replace_all {
            let count = content.matches(&edit.old).count();
            if count == 0 {
                return Err(format!(
                    "edit #{position}: oldText not found (replace_all was requested)"
                ));
            }
            content = content.replace(&edit.old, &edit.new);
            continue;
        }

        match content.matches(&edit.old).count() {
            1 => {
                content = content.replacen(&edit.old, &edit.new, 1);
            }
            0 => {
                content = fuzzy_replace_once(&content, &edit.old, &edit.new)
                    .map_err(|error| format!("edit #{position}: {error}"))?;
            }
            count => {
                return Err(format!(
                    "edit #{position}: oldText matches {count} times; include more context to make it unique"
                ));
            }
        }
    }

    Ok(content)
}

/// Strip `N→` line-number prefixes if a model copied lines verbatim from `read` output.
fn clean_line_numbers(edit: &Edit) -> Edit {
    let (old, stripped_old) = strip_read_gutter(&edit.old);
    let new = if stripped_old {
        strip_read_gutter(&edit.new).0
    } else {
        edit.new.clone()
    };
    Edit {
        old,
        new,
        replace_all: edit.replace_all,
    }
}

fn strip_read_gutter(text: &str) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return (text.to_string(), false);
    }
    let all_numbered = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| split_gutter_line(l).is_some());
    if !all_numbered {
        return (text.to_string(), false);
    }
    let mut out: String = lines
        .iter()
        .map(|l| split_gutter_line(l).unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    (out, true)
}

fn split_gutter_line(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    let arrow = trimmed.find('→')?;
    if arrow > 0 && trimmed[..arrow].bytes().all(|b| b.is_ascii_digit()) {
        Some(&trimmed[arrow + '→'.len_utf8()..])
    } else {
        None
    }
}

/// Match ignoring leading/trailing whitespace on each line. Requires a unique
/// match so an ambiguous edit never silently changes the wrong place.
fn fuzzy_replace_once(content: &str, old: &str, new: &str) -> Result<String, String> {
    let needle: Vec<&str> = old
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if needle.is_empty() {
        return Err("oldText has no non-empty lines".into());
    }

    let hay: Vec<&str> = content.lines().map(str::trim).collect();
    if hay.len() < needle.len() {
        return Err("oldText not found (exact or whitespace-insensitive)".into());
    }

    let mut starts = Vec::new();
    for start in 0..=(hay.len() - needle.len()) {
        if needle
            .iter()
            .enumerate()
            .all(|(offset, line)| hay[start + offset] == *line)
        {
            starts.push(start);
        }
    }

    match starts.len() {
        0 => Err("oldText not found (exact or whitespace-insensitive)".into()),
        1 => {
            let start_line = starts[0];
            let mut line_starts = Vec::with_capacity(hay.len());
            let mut offset = 0;
            for line in content.split_inclusive('\n') {
                line_starts.push(offset);
                offset += line.len();
            }

            let start_byte = line_starts[start_line];
            let end_byte = line_starts
                .get(start_line + needle.len())
                .copied()
                .unwrap_or(content.len());

            let mut replacement = new.to_string();
            if end_byte < content.len() && !replacement.ends_with('\n') {
                replacement.push('\n');
            }

            let mut result = String::with_capacity(content.len());
            result.push_str(&content[..start_byte]);
            result.push_str(&replacement);
            result.push_str(&content[end_byte..]);
            Ok(result)
        }
        count => Err(format!(
            "oldText matches {count} locations after whitespace normalisation; include more context"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            old: old.into(),
            new: new.into(),
            replace_all: false,
        }
    }

    #[test]
    fn replaces_unique_match() {
        let out = apply_edits("a\nb\nc\n", &[edit("b", "B")]).unwrap();
        assert_eq!(out, "a\nB\nc\n");
    }

    #[test]
    fn rejects_ambiguous_match() {
        let error = apply_edits("x\nx\n", &[edit("x", "y")]).unwrap_err();
        assert!(error.contains("matches 2 times"));
    }

    #[test]
    fn replace_all_works() {
        let mut e = edit("x", "y");
        e.replace_all = true;
        let out = apply_edits("x x x", &[e]).unwrap();
        assert_eq!(out, "y y y");
    }

    #[test]
    fn fuzzy_ignores_indentation() {
        let out = apply_edits("fn main() {\n    let a = 1;\n}\n", &[edit("let a = 1;", "let a = 2;")])
            .unwrap();
        assert!(out.contains("let a = 2;"));
    }

    #[test]
    fn fuzzy_rejects_duplicates() {
        let content = "  a\n  a\n";
        let error = apply_edits(content, &[edit("a", "b")]).unwrap_err();
        assert!(error.contains("matches 2 times"));
    }

    #[test]
    fn applies_multiple_edits_in_order() {
        let out = apply_edits("one two", &[edit("one", "1"), edit("two", "2")]).unwrap();
        assert_eq!(out, "1 2");
    }

    #[test]
    fn rejects_empty_old_text() {
        assert!(apply_edits("abc", &[edit("", "x")]).is_err());
    }

    #[test]
    fn missing_text_is_error() {
        assert!(apply_edits("abc", &[edit("zzz", "x")]).is_err());
    }

    #[test]
    fn strips_line_number_gutter_copied_from_read_tool() {
        let out = apply_edits(
            "fn a() {\n    let x = 1;\n}\n",
            &[edit("     2→    let x = 1;", "     2→    let x = 42;")],
        )
        .unwrap();
        assert_eq!(out, "fn a() {\n    let x = 42;\n}\n");
    }
}
