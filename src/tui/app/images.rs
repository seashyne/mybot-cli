use std::path::Path;
use std::process::Stdio;

use tokio_util::sync::CancellationToken;

use crate::protocol::{ImageAttachment, encode_base64};
use crate::tools::resolve_path;

use super::{App, Ctx, Entry};

const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

pub(crate) fn media_type_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

pub(crate) fn clean_pasted_path(raw: &str) -> String {
    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
        })
        .unwrap_or(trimmed);
    unquoted
        .strip_prefix("file://")
        .unwrap_or(unquoted)
        .replace("\\ ", " ")
}

pub(crate) fn load_image_file(cwd: &Path, raw: &str) -> Result<ImageAttachment, String> {
    let cleaned = clean_pasted_path(raw);
    if cleaned.is_empty() {
        return Err("missing image path".into());
    }
    let path = resolve_path(cwd, &cleaned);
    let media_type = media_type_for(&path)
        .ok_or_else(|| "unsupported image format (use png, jpg, webp, or gif)".to_string())?;
    let meta = std::fs::metadata(&path)
        .map_err(|e| format!("cannot access {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("not a file: {}", path.display()));
    }
    if meta.len() > MAX_IMAGE_BYTES as u64 {
        return Err(format!(
            "image too large ({} bytes, max {} MB)",
            meta.len(),
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image")
        .to_string();
    Ok(ImageAttachment {
        name,
        media_type: media_type.to_string(),
        data_base64: encode_base64(&bytes),
    })
}

/// Detect when pasted text (e.g. terminal drag-and-drop) is an existing image path.
pub(crate) fn try_load_pasted_image(
    cwd: &Path,
    pasted: &str,
) -> Option<Result<ImageAttachment, String>> {
    let trimmed = pasted.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }
    let cleaned = clean_pasted_path(trimmed);
    let path = resolve_path(cwd, &cleaned);
    media_type_for(&path)?;
    if !path.exists() {
        return None;
    }
    Some(load_image_file(cwd, trimmed))
}

pub(crate) fn read_clipboard_image() -> Result<ImageAttachment, String> {
    #[cfg(target_os = "linux")]
    {
        for (bin, args) in [
            ("wl-paste", vec!["--no-newline", "--type", "image/png"]),
            (
                "xclip",
                vec!["-selection", "clipboard", "-t", "image/png", "-o"],
            ),
        ] {
            if let Ok(output) = std::process::Command::new(bin)
                .args(&args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                && output.status.success()
                && output.stdout.starts_with(b"\x89PNG")
                && output.stdout.len() <= MAX_IMAGE_BYTES
            {
                return Ok(ImageAttachment {
                    name: "clipboard.png".into(),
                    media_type: "image/png".into(),
                    data_base64: encode_base64(&output.stdout),
                });
            }
        }
        Err("no PNG image in clipboard (or install wl-clipboard / xclip, or use /image <path>)".into())
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("pngpaste")
            .arg("-")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && output.status.success()
            && output.stdout.starts_with(b"\x89PNG")
            && output.stdout.len() <= MAX_IMAGE_BYTES
        {
            return Ok(ImageAttachment {
                name: "clipboard.png".into(),
                media_type: "image/png".into(),
                data_base64: encode_base64(&output.stdout),
            });
        }
        Err("no PNG image in clipboard (install pngpaste or use /image <path>)".into())
    }

    #[cfg(target_os = "windows")]
    {
        let script = "Add-Type -AssemblyName System.Windows.Forms; \
            $img = [System.Windows.Forms.Clipboard]::GetImage(); \
            if ($null -eq $img) { exit 1 }; \
            $ms = New-Object System.IO.MemoryStream; \
            $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); \
            [Console]::Write([Convert]::ToBase64String($ms.ToArray()))";
        if let Ok(output) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && output.status.success()
        {
            let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !b64.is_empty() && b64.len() <= MAX_IMAGE_BYTES * 2 {
                return Ok(ImageAttachment {
                    name: "clipboard.png".into(),
                    media_type: "image/png".into(),
                    data_base64: b64,
                });
            }
        }
        Err("no image in clipboard (use /image <path>)".into())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("clipboard image paste is not supported on this OS; use /image <path>".into())
    }
}

impl App {
    pub(crate) fn attach_image_path(&mut self, raw: &str) {
        match load_image_file(&self.cwd, raw) {
            Ok(img) => {
                let name = img.name.clone();
                self.pending_images.push(img);
                self.info(format!(
                    "attached {name} ({} total)",
                    self.pending_images.len()
                ));
            }
            Err(err) => self.info(err),
        }
    }

    pub(crate) fn attach_clipboard_image(&mut self) {
        match read_clipboard_image() {
            Ok(img) => {
                self.pending_images.push(img);
                self.info(format!(
                    "attached clipboard image ({} total)",
                    self.pending_images.len()
                ));
            }
            Err(err) => self.info(err),
        }
    }

    pub(super) fn image_command(&mut self, args: &str) {
        let arg = args.trim();
        if arg.is_empty() {
            if self.pending_images.is_empty() {
                self.info("usage: /image <path|paste|clear>");
            } else {
                let names: Vec<&str> = self
                    .pending_images
                    .iter()
                    .map(|img| img.name.as_str())
                    .collect();
                self.info(format!("attached: {}", names.join(", ")));
            }
            return;
        }
        if arg == "clear" {
            let count = self.pending_images.len();
            self.pending_images.clear();
            self.info(format!("cleared {count} attached image(s)"));
            return;
        }
        if arg == "paste" {
            self.attach_clipboard_image();
            return;
        }
        self.attach_image_path(arg);
    }

    pub(super) fn submit(&mut self, ctx: &Ctx) {
        if self.busy {
            self.info("wait for the current turn to finish (Esc cancels)");
            return;
        }

        let text = self.composer.take();
        let trimmed = text.trim();
        if trimmed.is_empty() && self.pending_images.is_empty() {
            return;
        }

        if trimmed.starts_with('/') {
            self.slash(trimmed, ctx);
            return;
        }

        let images = std::mem::take(&mut self.pending_images);
        let prompt = if trimmed.is_empty() {
            "Describe or inspect the attached image(s).".to_string()
        } else {
            trimmed.to_string()
        };
        let display = if images.is_empty() {
            prompt.clone()
        } else {
            let labels: Vec<String> = images.iter().map(|i| format!("[image: {}]", i.name)).collect();
            format!("{} {prompt}", labels.join(" "))
        };
        self.transcript.push(Entry::User(display));
        self.start_turn_with_images(prompt, images, ctx);
    }

    /// Send `prompt` to the model without touching the composer.
    pub(super) fn start_turn(&mut self, prompt: String, ctx: &Ctx) {
        self.start_turn_with_images(prompt, Vec::new(), ctx);
    }

    pub(super) fn start_turn_with_images(
        &mut self,
        prompt: String,
        images: Vec<ImageAttachment>,
        ctx: &Ctx,
    ) {
        if self.busy {
            self.info("wait for the current turn to finish (Esc cancels)");
            return;
        }

        self.busy = true;
        self.streaming_active = true;
        self.streaming.clear();
        self.thinking_chars = 0;
        self.cancel = CancellationToken::new();
        self.auto_scroll = true;
        self.scroll_from_bottom = 0;
        self.last_prompt = Some(prompt.clone());

        let agent = ctx.agent.clone();
        let tx = ctx.tx.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut agent = agent.lock().await;
            // Errors are reported through AgentEvent::Error.
            let _ = agent
                .run_turn_with_images(&prompt, images, &tx, &cancel)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_png_image_from_pasted_file_url() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("shot.png");
        std::fs::write(&file, b"\x89PNG\r\n\x1a\nhello").unwrap();

        let uri = format!("'file://{}'", file.display());
        let attached = try_load_pasted_image(dir.path(), &uri)
            .expect("should detect image path")
            .expect("should load png");
        assert_eq!(attached.name, "shot.png");
        assert_eq!(attached.media_type, "image/png");
        assert!(!attached.data_base64.is_empty());
    }

    #[test]
    fn ignores_non_image_or_missing_pasted_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(try_load_pasted_image(dir.path(), "src/main.rs").is_none());
        assert!(try_load_pasted_image(dir.path(), "missing.png").is_none());
    }
}
