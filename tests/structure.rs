use std::path::{Path, PathBuf};

const LIMIT: usize = 400;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn source_files_stay_within_the_line_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for folder in ["src", "tests", "benches"] {
        rust_files(&root.join(folder), &mut files);
    }

    let mut violations = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines = text.lines().count();
        if lines > LIMIT {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            violations.push(format!("{}: {lines} lines", relative.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "keep every file at or below {LIMIT} lines; split by responsibility:\n{}",
        violations.join("\n")
    );
}
