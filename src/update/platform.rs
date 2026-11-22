use crate::error::{AppError, Result};
use crate::version;

const SUPPORTED: [&str; 5] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

/// Release asset name for the running build, for example
/// `mybot-x86_64-unknown-linux-gnu` or `mybot-x86_64-pc-windows-msvc.exe`.
pub(super) fn asset_name() -> Result<String> {
    asset_name_for(version::TARGET)
}

pub(super) fn asset_name_for(target: &str) -> Result<String> {
    if !SUPPORTED.contains(&target) {
        return Err(AppError::runtime(format!(
            "self-update is not available for target '{target}'; reinstall with install.sh or from source"
        )));
    }
    if target.contains("windows") {
        Ok(format!("mybot-{target}.exe"))
    } else {
        Ok(format!("mybot-{target}"))
    }
}
