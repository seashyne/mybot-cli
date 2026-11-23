use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::error::{AppError, Result};

/// Write the downloaded binary next to `target`, probe it, then rename it over
/// `target`. The rename is atomic on Unix, so a running instance keeps the old
/// inode and the next start uses the new binary.
pub(super) fn replace(bytes: &[u8], target: &Path, expected_version: &str) -> Result<()> {
    let parent = target.parent().ok_or_else(|| {
        AppError::runtime(format!("cannot install next to {}", target.display()))
    })?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mybot");
    let staged = parent.join(format!(".{name}.new.{}", std::process::id()));

    write_executable(&staged, bytes).map_err(|error| {
        AppError::runtime(format!(
            "cannot write {}: {error}\nfix: rerun with sudo, or install to a writable prefix",
            staged.display()
        ))
    })?;

    if let Err(error) = probe(&staged, expected_version) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    if let Err(error) = replace_file(&staged, target) {
        let _ = fs::remove_file(&staged);
        return Err(AppError::runtime(format!(
            "cannot replace {}: {error}\nfix: rerun with sudo, or install to a writable prefix",
            target.display()
        )));
    }
    Ok(())
}

fn write_executable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn probe(path: &Path, expected_version: &str) -> Result<()> {
    let output = run_probe(path).map_err(|error| {
        AppError::runtime(format!("downloaded binary cannot run: {error}"))
    })?;
    if !output.status.success() {
        return Err(AppError::runtime(format!(
            "downloaded binary exited with {} when probed",
            output.status
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported = stdout
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("mybot "))
        .and_then(|rest| rest.split_whitespace().next());
    if reported != Some(expected_version) {
        return Err(AppError::runtime(format!(
            "downloaded binary reported an unexpected version: {}",
            stdout.trim()
        )));
    }
    Ok(())
}

fn run_probe(path: &Path) -> std::io::Result<std::process::Output> {
    // On Unix, a concurrent fork() in another thread can briefly hold the staged
    // file descriptor open and trigger ETXTBSY until exec/close completes.
    for attempt in 0..8 {
        match Command::new(path).arg("--version").output() {
            Ok(output) => return Ok(output),
            Err(err) if is_text_busy(&err) && attempt < 7 => {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!()
}

#[cfg(unix)]
fn is_text_busy(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn is_text_busy(_err: &std::io::Error) -> bool {
    false
}

#[cfg(unix)]
fn replace_file(staged: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(staged, target)
}

#[cfg(not(unix))]
fn replace_file(staged: &Path, target: &Path) -> std::io::Result<()> {
    let backup = target.with_extension("old");
    let _ = fs::remove_file(&backup);
    fs::rename(target, &backup)?;
    fs::rename(staged, target)?;
    let _ = fs::remove_file(&backup);
    Ok(())
}
