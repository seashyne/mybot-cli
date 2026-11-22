use crate::cli::Cli;
use crate::error::Result;

use super::{system_parts, working_dir};

pub(super) async fn run(cli: &Cli) -> Result<()> {
    let cwd = working_dir(cli)?;
    let (system_override, append_system) = system_parts(cli)?;
    crate::tui::run(crate::tui::TuiOptions {
        cwd,
        model: cli.model.clone(),
        permission: cli.permission_mode()?,
        system_override,
        append_system,
        session_id: cli.session.clone(),
        continue_: cli.continue_,
        initial_prompt: cli.prompt.clone(),
    })
    .await
}
