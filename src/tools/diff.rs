use std::path::Path;

use serde_json::Value;

use super::{arg_str, resolve_path};

const MAX_WRITE_DIFF_BYTES: u64 = 256 * 1024;
const MAX_LINES: usize = 18;

/// Build a concise diff preview for `edit` or `write` permission prompts.
pub fn preview(tool: &str, args: &Value, cwd: &Path) -> Option<String> {
    match tool {
        "edit" => preview_edit(args),
        "write" => preview_write(args, cwd),
        _ => None,
    }
}

fn preview_edit(args: &Value) -> Option<String> {
    let path = arg_str(args, "path")
        .or_else(|| arg_str(args, "file_path"))
        .unwrap_or("?");
    let mut lines = vec![format!("--- {path}"), format!("+++ {path}")];

    if let Some(list) = args.get("edits").and_then(Value::as_array) {
        for (index, item) in list.iter().enumerate() {
            let old = item.get("oldText").and_then(Value::as_str).unwrap_or("");
            let new = item.get("newText").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("@@ edit {} @@", index + 1));
            push_prefixed(&mut lines, '-', old);
            push_prefixed(&mut lines, '+', new);
            if lines.len() >= MAX_LINES {
                break;
            }
        }
    } else if let (Some(old), Some(new)) = (
        args.get("old_string").and_then(Value::as_str),
        args.get("new_string").and_then(Value::as_str),
    ) {
        push_prefixed(&mut lines, '-', old);
        push_prefixed(&mut lines, '+', new);
    } else {
        return Some(format!("edit {path}"));
    }

    Some(cap_lines(lines, MAX_LINES))
}

fn preview_write(args: &Value, cwd: &Path) -> Option<String> {
    let raw_path = arg_str(args, "path").unwrap_or("?");
    let content = arg_str(args, "content").unwrap_or("");
    let full_path = resolve_path(cwd, raw_path);

    let existing = std::fs::metadata(&full_path)
        .ok()
        .filter(|meta| meta.len() <= MAX_WRITE_DIFF_BYTES)
        .and_then(|_| std::fs::read_to_string(&full_path).ok());

    let mut lines = match existing {
        None => {
            let mut out = vec![
                format!("+++ {raw_path} (new file, {} bytes)", content.len()),
            ];
            push_prefixed(&mut out, '+', content);
            out
        }
        Some(old) => {
            let mut out = vec![
                format!("--- {raw_path}"),
                format!("+++ {raw_path} ({} bytes)", content.len()),
            ];
            let old_lines: Vec<&str> = old.lines().collect();
            let new_lines: Vec<&str> = content.lines().collect();
            let prefix = old_lines
                .iter()
                .zip(new_lines.iter())
                .take_while(|(a, b)| a == b)
                .count();
            let max_suffix = old_lines
                .len()
                .saturating_sub(prefix)
                .min(new_lines.len().saturating_sub(prefix));
            let suffix = old_lines
                .iter()
                .rev()
                .zip(new_lines.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| a == b)
                .count();

            let old_slice = &old_lines[prefix..old_lines.len() - suffix];
            let new_slice = &new_lines[prefix..new_lines.len() - suffix];
            if old_slice.is_empty() && new_slice.is_empty() {
                out.push("  (no content changes)".into());
            } else {
                out.push(format!("@@ line {} @@", prefix + 1));
                for line in old_slice.iter().take(6) {
                    out.push(format!("- {line}"));
                }
                if old_slice.len() > 6 {
                    out.push(format!("- … ({} more removed lines)", old_slice.len() - 6));
                }
                for line in new_slice.iter().take(8) {
                    out.push(format!("+ {line}"));
                }
                if new_slice.len() > 8 {
                    out.push(format!("+ … ({} more added lines)", new_slice.len() - 8));
                }
            }
            out
        }
    };

    if lines.is_empty() {
        lines.push(format!("write {raw_path} ({} bytes)", content.len()));
    }
    Some(cap_lines(lines, MAX_LINES))
}

fn push_prefixed(out: &mut Vec<String>, prefix: char, text: &str) {
    if text.is_empty() {
        return;
    }
    for line in text.lines().take(8) {
        out.push(format!("{prefix} {line}"));
    }
    let count = text.lines().count();
    if count > 8 {
        out.push(format!("{prefix} … ({} more lines)", count - 8));
    }
}

fn cap_lines(lines: Vec<String>, max: usize) -> String {
    if lines.len() <= max {
        return lines.join("\n");
    }
    let extra = lines.len() - (max - 1);
    let mut kept = lines[..max - 1].to_vec();
    kept.push(format!("… ({extra} more lines)"));
    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edit_preview_shows_removed_and_added_lines() {
        let dir = tempfile::tempdir().unwrap();
        let out = preview(
            "edit",
            &json!({
                "path": "src/lib.rs",
                "edits": [{"oldText": "let x = 1;", "newText": "let x = 2;"}]
            }),
            dir.path(),
        )
        .unwrap();
        assert!(out.contains("--- src/lib.rs"));
        assert!(out.contains("- let x = 1;"));
        assert!(out.contains("+ let x = 2;"));
    }

    #[test]
    fn write_preview_diffs_against_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "line 1\nold\nline 3\n").unwrap();
        let out = preview(
            "write",
            &json!({"path": "a.txt", "content": "line 1\nnew\nline 3\n"}),
            dir.path(),
        )
        .unwrap();
        assert!(out.contains("@@ line 2 @@"));
        assert!(out.contains("- old"));
        assert!(out.contains("+ new"));
    }
}
