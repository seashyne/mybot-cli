use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::protocol::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    #[serde(default = "default_version")]
    pub v: u32,
    pub id: String,
    pub cwd: String,
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Entry {
    Header(SessionHeader),
    Message { message: Message },
}

pub struct Session {
    pub header: SessionHeader,
    pub messages: Vec<Message>,
    pub path: PathBuf,
}

impl Session {
    pub fn create(cwd: &Path, provider: &str, model: &str) -> crate::error::Result<Self> {
        crate::config::ensure_dirs()?;
        let header = SessionHeader {
            v: 1,
            id: uuid::Uuid::new_v4().to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            created_at: Utc::now(),
        };
        let path = crate::config::sessions_dir().join(format!("{}.jsonl", header.id));
        let line = serde_json::to_string(&Entry::Header(header.clone()))?;
        std::fs::write(&path, format!("{line}\n"))?;
        Ok(Self {
            header,
            messages: Vec::new(),
            path,
        })
    }

    pub fn open(id: &str) -> crate::error::Result<Self> {
        let path = find_session_file(id)?;
        let file = std::fs::File::open(&path)?;
        let reader = BufReader::new(file);

        let mut header = None;
        let mut messages = Vec::new();
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if line.trim().is_empty() {
                continue;
            }
            // A torn final line is expected after a crash; skip anything unparsable.
            match serde_json::from_str::<Entry>(&line) {
                Ok(Entry::Header(value)) => header = Some(value),
                Ok(Entry::Message { message }) => messages.push(message),
                Err(_) => continue,
            }
        }

        let header = header.ok_or_else(|| {
            crate::error::AppError::config(format!("session '{id}' has no valid header"))
        })?;

        Ok(Self {
            header,
            messages,
            path,
        })
    }

    /// Append a message to the session ledger. Does not push into `messages`.
    pub fn append(&mut self, message: &Message) -> crate::error::Result<()> {
        let line = serde_json::to_string(&Entry::Message {
            message: message.clone(),
        })?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub fn list() -> crate::error::Result<Vec<SessionHeader>> {
        let dir = crate::config::sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut headers = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(header) = read_header(&path) {
                headers.push(header);
            }
        }
        headers.sort_by_key(|header| std::cmp::Reverse(header.created_at));
        Ok(headers)
    }

    pub fn latest_for_cwd(cwd: &Path) -> crate::error::Result<Option<String>> {
        let target = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        for header in Self::list()? {
            let session_cwd = Path::new(&header.cwd);
            let session_cwd = session_cwd
                .canonicalize()
                .unwrap_or_else(|_| session_cwd.to_path_buf());
            if session_cwd == target {
                return Ok(Some(header.id));
            }
        }
        Ok(None)
    }

    pub fn id(&self) -> &str {
        &self.header.id
    }

    /// First user message, trimmed to one short line.
    pub fn title(&self) -> String {
        for message in &self.messages {
            if let Message::User { content } = message {
                let first_line = content.lines().next().unwrap_or_default().trim();
                if !first_line.is_empty() {
                    return first_line.chars().take(60).collect();
                }
            }
        }
        "(empty session)".to_string()
    }
}

fn find_session_file(id: &str) -> crate::error::Result<PathBuf> {
    let dir = crate::config::sessions_dir();
    let exact = dir.join(format!("{id}.jsonl"));
    if exact.exists() {
        return Ok(exact);
    }

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(id) && name.ends_with(".jsonl") {
                return Ok(entry.path());
            }
        }
    }

    Err(crate::error::AppError::config(format!(
        "session '{id}' not found in {}",
        dir.display()
    )))
}

fn read_header(path: &Path) -> Option<SessionHeader> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    match serde_json::from_str::<Entry>(&line) {
        Ok(Entry::Header(header)) => Some(header),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _dir: tempfile::TempDir,
        _lock: MutexGuard<'static, ()>,
    }

    fn with_home() -> EnvGuard {
        let lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: ENV_LOCK serialises every test that touches MYBOT_HOME.
        unsafe {
            std::env::set_var("MYBOT_HOME", dir.path());
        }
        EnvGuard {
            _dir: dir,
            _lock: lock,
        }
    }

    #[test]
    fn round_trip_messages() {
        let _guard = with_home();
        let cwd = std::env::temp_dir();
        let mut session = Session::create(&cwd, "mock", "mock-model").unwrap();
        let id = session.id().to_string();

        session.append(&Message::user("hello")).unwrap();
        session
            .append(&Message::assistant_text("hi there"))
            .unwrap();

        let loaded = Session::open(&id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.title(), "hello");
        assert_eq!(loaded.header.provider, "mock");
    }

    #[test]
    fn tolerates_corrupt_tail() {
        let _guard = with_home();
        let cwd = std::env::temp_dir();
        let mut session = Session::create(&cwd, "mock", "mock-model").unwrap();
        session.append(&Message::user("one")).unwrap();
        let id = session.id().to_string();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&session.path)
            .unwrap();
        writeln!(file, "{{not json").unwrap();

        let loaded = Session::open(&id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn latest_for_cwd_finds_session() {
        let _guard = with_home();
        let dir = tempfile::tempdir().unwrap();
        let session = Session::create(dir.path(), "mock", "m").unwrap();
        let latest = Session::latest_for_cwd(dir.path()).unwrap();
        assert_eq!(latest.as_deref(), Some(session.id()));
    }

    #[test]
    fn missing_session_errors() {
        let _guard = with_home();
        assert!(Session::open("does-not-exist").is_err());
    }
}
