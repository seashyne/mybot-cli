use std::path::PathBuf;

/// Root data directory: `$MYBOT_HOME` or `~/.mybot`.
pub fn data_dir() -> PathBuf {
    if let Ok(value) = std::env::var("MYBOT_HOME")
        && !value.is_empty() {
            return PathBuf::from(value);
        }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mybot")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

pub fn log_path() -> PathBuf {
    data_dir().join("mybot.log")
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(sessions_dir())?;
    Ok(())
}
