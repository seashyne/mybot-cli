//! Self-update: resolve a GitHub release, verify its checksum, and replace
//! the running binary in place.

mod download;
mod install;
mod platform;
mod release;
#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, Result};
use crate::version;

pub struct UpdateOptions {
    pub check: bool,
    pub force: bool,
    pub tag: Option<String>,
}

#[derive(Debug)]
pub enum UpdateOutcome {
    UpToDate {
        current: String,
        latest: String,
    },
    Available {
        current: String,
        latest: String,
        url: String,
        published_at: Option<String>,
    },
    Installed {
        from: String,
        to: String,
        path: PathBuf,
    },
}

impl UpdateOutcome {
    pub fn message(&self) -> String {
        match self {
            Self::UpToDate { current, latest } => {
                format!("mybot {current} is up to date (latest release: {latest})")
            }
            Self::Available {
                current,
                latest,
                url,
                published_at,
            } => {
                let date = published_at
                    .as_deref()
                    .map(|date| format!(", published {date}"))
                    .unwrap_or_default();
                format!(
                    "mybot {latest} is available (current {current}{date})\n{url}\nrun `mybot update` to install"
                )
            }
            Self::Installed { from, to, path } => format!(
                "updated mybot {from} → {to}\ninstalled at {}\nrestart running sessions to use the new binary",
                path.display()
            ),
        }
    }
}

pub async fn run(options: UpdateOptions) -> Result<UpdateOutcome> {
    let release = release::fetch(options.tag.as_deref()).await?;
    let current = semver::Version::parse(version::VERSION)
        .map_err(|error| AppError::runtime(format!("current version is not semver: {error}")))?;
    let latest = release.version.clone();

    if !options.force && latest <= current {
        return Ok(UpdateOutcome::UpToDate {
            current: current.to_string(),
            latest: latest.to_string(),
        });
    }

    if options.check {
        return Ok(UpdateOutcome::Available {
            current: current.to_string(),
            latest: latest.to_string(),
            url: release.url.clone(),
            published_at: release.published_at.clone(),
        });
    }

    let asset_name = platform::asset_name()?;
    let asset = release.asset(&asset_name).ok_or_else(|| {
        AppError::runtime(format!(
            "release {} has no {} asset (available: {})",
            release.tag,
            asset_name,
            release.asset_names().join(", ")
        ))
    })?;
    let sums = release.asset("sha256sums.txt").ok_or_else(|| {
        AppError::runtime(format!(
            "release {} has no sha256sums.txt; refusing to update without a checksum",
            release.tag
        ))
    })?;

    let exe = version::binary_path()
        .ok_or_else(|| AppError::runtime("cannot locate the running binary"))?;
    if is_npm_managed(&exe) && !options.force {
        return Err(AppError::usage(
            "this binary is managed by npm; run `npm update -g mybot-cli` instead (or pass --force)",
        ));
    }
    ensure_writable(&exe)?;

    let client = http_client()?;
    eprintln!(
        "downloading {} ({}) from {}…",
        asset.name,
        human_size(asset.size),
        release.tag
    );
    let bytes = download::fetch(&client, &asset.browser_download_url).await?;
    let sums_text = download::fetch_text(&client, &sums.browser_download_url).await?;
    let expected = download::expected_hash(&sums_text, &asset_name).ok_or_else(|| {
        AppError::runtime(format!(
            "sha256sums.txt has no entry for {asset_name}; refusing to update"
        ))
    })?;
    download::verify(&bytes, &expected)?;
    eprintln!("checksum ok");

    eprintln!("installing to {}…", exe.display());
    install::replace(&bytes, &exe, &latest.to_string())?;

    Ok(UpdateOutcome::Installed {
        from: current.to_string(),
        to: latest.to_string(),
        path: exe,
    })
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!("mybot/{}", version::VERSION))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| AppError::runtime(format!("cannot build http client: {error}")))
}

fn is_npm_managed(path: &std::path::Path) -> bool {
    path.to_string_lossy().contains("node_modules")
}

/// Fail before downloading when the binary directory is not writable.
fn ensure_writable(target: &Path) -> Result<()> {
    let parent = target.parent().ok_or_else(|| {
        AppError::runtime(format!("cannot install next to {}", target.display()))
    })?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mybot");
    let probe = parent.join(format!(".{name}.write-test.{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(error) => Err(AppError::runtime(format!(
            "cannot write to {}: {error}\nfix: rerun with sudo, or install to a writable prefix",
            parent.display()
        ))),
    }
}

fn human_size(bytes: u64) -> String {
    if bytes == 0 {
        return "size unknown".to_string();
    }
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
