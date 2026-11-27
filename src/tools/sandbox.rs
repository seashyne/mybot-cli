use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;

use tokio::process::Command;

use crate::config::SandboxMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Bubblewrap,
    Seatbelt,
    None,
}

impl SandboxBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bubblewrap => "bubblewrap",
            Self::Seatbelt => "sandbox-exec",
            Self::None => "none",
        }
    }
}

static DETECTED: OnceLock<SandboxBackend> = OnceLock::new();

/// Probe once whether an OS sandbox backend can actually run in this environment.
pub fn detected_backend() -> SandboxBackend {
    *DETECTED.get_or_init(probe_backend)
}

fn probe_backend() -> SandboxBackend {
    #[cfg(target_os = "linux")]
    {
        if std::process::Command::new("bwrap")
            .args([
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--die-with-parent",
                "true",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return SandboxBackend::Bubblewrap;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if std::process::Command::new("sandbox-exec")
            .args(["-p", "(version 1)(allow default)", "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return SandboxBackend::Seatbelt;
        }
    }

    SandboxBackend::None
}

/// Build a shell `Command` wrapped in the active OS sandbox policy.
pub fn build_command(shell_script: &str, cwd: &Path, mode: SandboxMode) -> Result<Command, String> {
    let backend = match mode {
        SandboxMode::Off => SandboxBackend::None,
        SandboxMode::Auto => detected_backend(),
        SandboxMode::Strict => {
            let found = detected_backend();
            if found == SandboxBackend::None {
                return Err(
                    "sandbox mode is 'strict' but no OS sandbox (bwrap on Linux, sandbox-exec on macOS) is usable here"
                        .to_string(),
                );
            }
            found
        }
    };

    let mut cmd = match backend {
        SandboxBackend::Bubblewrap => build_bwrap(shell_script, cwd, mode),
        SandboxBackend::Seatbelt => build_seatbelt(shell_script, cwd, mode),
        SandboxBackend::None => build_direct(shell_script),
    };

    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("MYBOT", "1");

    #[cfg(unix)]
    cmd.process_group(0);

    Ok(cmd)
}

fn build_direct(shell_script: &str) -> Command {
    #[cfg(windows)]
    {
        if let Some(bash) = find_windows_bash() {
            let mut cmd = Command::new(bash);
            cmd.arg("-lc").arg(shell_script);
            return cmd;
        }
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", shell_script]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("bash");
        cmd.arg("-lc").arg(shell_script);
        cmd
    }
}

#[cfg(windows)]
fn find_windows_bash() -> Option<&'static str> {
    [
        "bash",
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    .into_iter()
    .find(|&candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

#[allow(dead_code)]
fn writable_cache_paths() -> Vec<std::path::PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        ".cache",
        ".npm",
        ".cargo/registry",
        ".cargo/git",
        ".cargo/.package-cache",
    ]
    .into_iter()
    .map(|rel| home.join(rel))
    .filter(|path| path.exists())
    .collect()
}

#[allow(dead_code)]
fn build_bwrap(shell_script: &str, cwd: &Path, mode: SandboxMode) -> Command {
    let mut cmd = Command::new("bwrap");
    cmd.args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    if mode == SandboxMode::Strict {
        cmd.arg("--unshare-net");
    }
    if Path::new("/tmp").exists() {
        cmd.args(["--bind", "/tmp", "/tmp"]);
    }
    for cache in writable_cache_paths() {
        let s = cache.to_string_lossy().to_string();
        cmd.args(["--bind", &s, &s]);
    }

    let cwd_str = cwd.to_string_lossy().to_string();
    cmd.args(["--bind", &cwd_str, &cwd_str]);

    // Mount tmpfs over ~/.mybot after binding cwd so API keys stay hidden even
    // when mybot is launched directly from $HOME.
    let data_dir = crate::config::data_dir();
    if data_dir.exists() && !cwd.starts_with(&data_dir) {
        cmd.arg("--tmpfs").arg(&data_dir);
    }

    cmd.arg("--die-with-parent");
    cmd.args(["bash", "-lc", shell_script]);
    cmd
}

#[allow(dead_code)]
fn build_seatbelt(shell_script: &str, cwd: &Path, mode: SandboxMode) -> Command {
    let cwd_escaped = escape_seatbelt(cwd);
    let extra_caches: String = writable_cache_paths()
        .iter()
        .map(|p| format!(" (subpath \"{}\")", escape_seatbelt(p)))
        .collect();
    let data_dir = crate::config::data_dir();
    let hide_data = if !cwd.starts_with(&data_dir) {
        format!("(deny file-read* (subpath \"{}\"))", escape_seatbelt(&data_dir))
    } else {
        String::new()
    };
    let deny_net = if mode == SandboxMode::Strict {
        "(deny network*)"
    } else {
        ""
    };
    let profile = format!(
        "(version 1)\
         (allow default)\
         (deny file-write* (subpath \"/\"))\
         (allow file-write* \
           (subpath \"{cwd_escaped}\") \
           (subpath \"/tmp\") \
           (subpath \"/private/tmp\") \
           (subpath \"/private/var/folders\") \
           (subpath \"/dev\"){extra_caches})\
         {hide_data}{deny_net}"
    );

    let mut cmd = Command::new("sandbox-exec");
    cmd.args(["-p", &profile, "bash", "-lc", shell_script]);
    cmd
}

#[allow(dead_code)]
fn escape_seatbelt(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_command_works_under_off_mode() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = build_command("echo ok", dir.path(), SandboxMode::Off);
        assert!(cmd.is_ok());
    }

    #[tokio::test]
    async fn bwrap_blocks_writes_outside_workspace_when_available() {
        if detected_backend() != SandboxBackend::Bubblewrap {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = build_command(
            "touch /etc/mybot-sandbox-probe 2>&1 || true",
            dir.path(),
            SandboxMode::Strict,
        )
        .unwrap();
        let output = cmd.output().await.unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Read-only") || stdout.contains("Permission denied"),
            "unexpected output: {stdout}"
        );
    }

    #[tokio::test]
    async fn strict_bwrap_isolates_network_namespace_when_available() {
        if detected_backend() != SandboxBackend::Bubblewrap {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = build_command(
            "cat /proc/net/route | wc -l",
            dir.path(),
            SandboxMode::Strict,
        )
        .unwrap();
        let output = cmd.output().await.unwrap();
        let lines: usize = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(99);
        // An unshared empty network namespace has only the header line in /proc/net/route.
        assert_eq!(lines, 1, "expected isolated netns with no routes");
    }
}
