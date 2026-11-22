use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_CHECKPOINTS: usize = 25;
const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub display: String,
    /// `None` means the file did not exist prior to the turn.
    pub previous: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct TurnCheckpoint {
    pub label: String,
    pub files: Vec<FileSnapshot>,
}

#[derive(Debug, Clone)]
pub struct UndoReport {
    pub label: String,
    pub restored: Vec<String>,
    pub removed: Vec<String>,
}

/// Records file snapshots before `write` and `edit` mutate the workspace so
/// `/undo` can revert the most recent turn cleanly.
#[derive(Debug, Clone, Default)]
pub struct CheckpointStore {
    turns: Arc<Mutex<Vec<TurnCheckpoint>>>,
}

impl CheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new checkpoint group for the upcoming user turn.
    pub fn begin_turn(&self, prompt: &str) {
        let mut guard = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if guard.last().is_some_and(|turn| turn.files.is_empty()) {
            guard.pop();
        }
        let label: String = prompt.trim().lines().next().unwrap_or("").chars().take(48).collect();
        guard.push(TurnCheckpoint {
            label,
            files: Vec::new(),
        });
        if guard.len() > MAX_CHECKPOINTS {
            let overflow = guard.len() - MAX_CHECKPOINTS;
            guard.drain(..overflow);
        }
    }

    /// Capture the pre-mutation state of `path` if not already recorded in the current turn.
    pub fn record_before_write(&self, path: &Path, display: String) {
        let mut guard = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_empty() {
            guard.push(TurnCheckpoint {
                label: String::new(),
                files: Vec::new(),
            });
        }
        let current = guard.last_mut().expect("non-empty turns");
        if current.files.iter().any(|snap| snap.path == path) {
            return;
        }

        let previous = if path.exists() {
            match std::fs::metadata(path) {
                Ok(meta) if meta.len() <= MAX_SNAPSHOT_BYTES => std::fs::read(path).ok(),
                _ => return,
            }
        } else {
            None
        };

        current.files.push(FileSnapshot {
            path: path.to_path_buf(),
            display,
            previous,
        });
    }

    pub fn undo_count(&self) -> usize {
        self.turns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|turn| !turn.files.is_empty())
            .count()
    }

    /// Revert all file changes recorded in the most recent non-empty turn.
    pub fn undo_last(&self) -> Result<Option<UndoReport>, String> {
        let turn = {
            let mut guard = self.turns.lock().unwrap_or_else(|e| e.into_inner());
            while guard.last().is_some_and(|turn| turn.files.is_empty()) {
                guard.pop();
            }
            guard.pop()
        };

        let Some(turn) = turn else {
            return Ok(None);
        };

        let mut restored = Vec::new();
        let mut removed = Vec::new();

        for snapshot in turn.files.into_iter().rev() {
            match snapshot.previous {
                Some(bytes) => {
                    if let Some(parent) = snapshot.path.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("cannot recreate {}: {e}", parent.display())
                        })?;
                    }
                    std::fs::write(&snapshot.path, &bytes).map_err(|e| {
                        format!("cannot restore {}: {e}", snapshot.display)
                    })?;
                    restored.push(snapshot.display);
                }
                None => {
                    if snapshot.path.exists() {
                        std::fs::remove_file(&snapshot.path).map_err(|e| {
                            format!("cannot remove {}: {e}", snapshot.display)
                        })?;
                    }
                    removed.push(snapshot.display);
                }
            }
        }

        restored.reverse();
        removed.reverse();
        Ok(Some(UndoReport {
            label: turn.label,
            restored,
            removed,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_first_snapshot_per_turn_and_reverts_on_undo() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("a.txt");
        let created = dir.path().join("b.txt");
        std::fs::write(&existing, "v1").unwrap();

        let store = CheckpointStore::new();
        store.begin_turn("update files");

        store.record_before_write(&existing, "a.txt".into());
        std::fs::write(&existing, "v2").unwrap();
        // Second write in the same turn must keep "v1", not "v2".
        store.record_before_write(&existing, "a.txt".into());
        std::fs::write(&existing, "v3").unwrap();

        store.record_before_write(&created, "b.txt".into());
        std::fs::write(&created, "new").unwrap();

        assert_eq!(store.undo_count(), 1);
        let report = store.undo_last().unwrap().expect("report");
        assert_eq!(report.restored, vec!["a.txt"]);
        assert_eq!(report.removed, vec!["b.txt"]);
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "v1");
        assert!(!created.exists());
        assert_eq!(store.undo_count(), 0);
    }
}
