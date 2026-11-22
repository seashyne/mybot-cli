use crate::error::{AppError, Result};
use crate::version;

const SUPPORTED: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// Release asset name for the running build, for example
/// `mybot-x86_64-unknown-linux-gnu`.
pub(super) fn asset_name() -> Result<String> {
    asset_name_for(version::TARGET)
}

pub(super) fn asset_name_for(target: &str) -> Result<String> {
    if !SUPPORTED.contains(&target) {
        return Err(AppError::runtime(format!(
            "self-update is not available for target '{target}'; reinstall with install.sh or from source"
        )));
    }
    Ok(format!("mybot-{target}"))
}
