use super::{App, Ctx};

mod session;
mod settings;
mod transcript;
mod view;

impl App {
    pub(super) fn slash(&mut self, command: &str, ctx: &Ctx) {
        let (name, args) = command.split_once(' ').unwrap_or((command, ""));
        let args = args.trim();

        match name {
            "/quit" | "/exit" => self.quit = true,
            "/help" => self.help_command(),
            "/changelog" => self.changelog_command(),
            "/new" | "/clear" => self.new_session(ctx),
            "/model" => self.model_command(args, ctx),
            "/effort" | "/thinking" => self.effort_command(args, ctx),
            "/permissions" | "/permission" | "/mode" => self.permissions_command(args, ctx),
            "/sandbox" => self.sandbox_command(args, ctx),
            "/image" | "/attach" => self.image_command(args),
            "/tools" => self.tools_command(ctx),
            "/context" | "/ctx" => self.context_command(ctx),
            "/status" => self.status_command(ctx),
            "/version" => self.version_command(),
            "/sessions" => self.sessions_command(),
            "/resume" => self.resume_command(args, ctx),
            "/compact" => self.compact_command(ctx),
            "/retry" => self.retry_command(ctx),
            "/undo" | "/rewind" => self.undo_command(ctx),
            "/stop" | "/cancel" => self.stop_command(),
            "/export" => self.export_command(args),
            "/copy" | "/yank" => self.copy_command(args),
            "/find" | "/search" => self.find_command(args),
            "/config" => self.config_command(),
            "/doctor" => self.doctor_command(ctx),
            "/keys" | "/hotkeys" => self.keys_command(),
            "/skills" => self.skills_command(ctx),
            "/skill" => self.skill_command(args, ctx),
            "/agents" => self.agents_command(ctx),
            "/memory" => self.memory_command(args),
            "/remember" => self.remember_command(args),
            "/mcp" => self.mcp_command(ctx),
            "/team" => self.team_command(),
            "/init" => self.init_command(),
            other => self.plugin_command(other, args, ctx),
        }
    }

    /// Commands that replace or rewind the conversation wait for the turn.
    pub(super) fn idle(&mut self) -> bool {
        if self.busy {
            self.info("wait for the current turn to finish (Esc or /stop cancels)");
            false
        } else {
            true
        }
    }
}
