#![cfg(all(unix, feature = "tui"))]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::Router;
use axum::response::IntoResponse;
use axum::routing::post;

const SSE_BODY: &str = concat!(
    "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello from PTY mock\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":5}}\n\n",
    "data: [DONE]\n\n",
);

async fn chat_handler() -> impl IntoResponse {
    ([("content-type", "text/event-stream")], SSE_BODY.to_string())
}

async fn start_mock_server() -> String {
    let app = Router::new().route("/v1/chat/completions", post(chat_handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}/v1")
}

struct Pty {
    master: std::fs::File,
    child: Child,
}

impl Pty {
    fn spawn(bin: &std::path::Path, home: &std::path::Path, cwd: &std::path::Path) -> Self {
        let mut master_fd: libc::c_int = -1;
        let mut slave_fd: libc::c_int = -1;
        let ws = libc::winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        #[cfg(target_os = "macos")]
        let ws_ptr = std::ptr::from_ref(&ws).cast_mut();
        #[cfg(not(target_os = "macos"))]
        let ws_ptr = std::ptr::from_ref(&ws);
        let rc = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                ws_ptr,
            )
        };
        assert_eq!(rc, 0, "openpty failed");

        let master = unsafe { OwnedFd::from_raw_fd(master_fd) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };
        let slave_raw = slave.as_raw_fd();

        // Set non-blocking mode on master so reads can poll with timeout.
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };

        let stdin_fd = slave.try_clone().unwrap();
        let stdout_fd = slave.try_clone().unwrap();
        let stderr_fd = slave.try_clone().unwrap();

        let mut cmd = Command::new(bin);
        cmd.current_dir(cwd)
            .env("MYBOT_HOME", home)
            .env("TERM", "xterm-256color")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .stdin(Stdio::from(stdin_fd))
            .stdout(Stdio::from(stdout_fd))
            .stderr(Stdio::from(stderr_fd));

        unsafe {
            cmd.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(slave_raw, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = cmd.spawn().expect("spawn mybot in PTY");
        drop(slave);

        Self {
            master: std::fs::File::from(master),
            child,
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
        self.master.flush().unwrap();
    }

    fn wait_for(&mut self, needle: &str, buf: &mut String, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; 4096];
        while Instant::now() < deadline {
            match self.master.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    if strip_ansi(buf).contains(needle) {
                        return true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        strip_ansi(buf).contains(needle)
    }

    fn wait_exit(&mut self, buf: &mut String, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; 4096];
        while Instant::now() < deadline {
            while let Ok(n) = self.master.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for mybot PTY exit:\n{}", strip_ansi(buf));
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    let mut final_byte = '\0';
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            final_byte = c;
                            break;
                        }
                    }
                    if matches!(final_byte, 'C' | 'G' | 'H') {
                        out.push(' ');
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                            if c == '\x1b' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        out.push(ch);
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn tui_renders_welcome_palette_reply_and_exits_cleanly_in_pty() {
    let base_url = start_mock_server().await;
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let config = format!(
        "default_provider = \"lmstudio\"\n\n[providers.lmstudio]\nkind = \"openai\"\nbase_url = \"{base_url}\"\nmodel = \"mock-gpt\"\nmax_tokens = 256\n"
    );
    std::fs::write(home.path().join("config.toml"), config).unwrap();

    #[allow(deprecated)]
    let bin = assert_cmd::cargo::cargo_bin("mybot");
    let mut pty = Pty::spawn(&bin, home.path(), cwd.path());
    let mut screen = String::new();

    assert!(
        pty.wait_for("Ask mybot anything", &mut screen, Duration::from_secs(5)),
        "expected welcome screen in PTY, got:\n{}",
        strip_ansi(&screen)
    );
    assert!(strip_ansi(&screen).contains("Ready"));

    // Open command palette with `/`, verify commands appear, then dismiss with Esc.
    screen.clear();
    pty.send(b"/");
    assert!(
        pty.wait_for("/tools", &mut screen, Duration::from_secs(3)),
        "expected /tools in command palette, got:\n{}",
        strip_ansi(&screen)
    );
    pty.send(b"\x1b");
    std::thread::sleep(Duration::from_millis(80));

    // Send a prompt and confirm streamed reply appears.
    pty.send(b"hi\r");
    assert!(
        pty.wait_for("Hello from PTY mock", &mut screen, Duration::from_secs(5)),
        "expected streamed assistant reply, got:\n{}",
        strip_ansi(&screen)
    );

    // Exit cleanly via `/quit`.
    pty.send(b"/quit\r");
    let status = pty.wait_exit(&mut screen, Duration::from_secs(5));
    assert!(status.success(), "expected clean exit, got {status}");
}
