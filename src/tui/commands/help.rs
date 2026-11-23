use super::{COMMANDS, Category, CommandSpec};

pub fn help_text() -> String {
    let mut text = String::from("commands:\n");
    for category in Category::ALL {
        let commands: Vec<&CommandSpec> = COMMANDS
            .iter()
            .filter(|command| command.category == category)
            .collect();
        if commands.is_empty() {
            continue;
        }
        text.push_str(&format!("\n{}\n", category.label()));
        for command in commands {
            let usage = if command.args.is_empty() {
                format!("/{}", command.name)
            } else {
                format!("/{} {}", command.name, command.args)
            };
            text.push_str(&format!(
                "  {:<30} {}\n",
                usage, command.description
            ));
        }
    }
    text.push_str(
        "\nkeys: Enter send · Shift+Enter newline · Esc cancel/clear · Ctrl+C cancel/exit\n\
         PgUp/PgDn scroll · Ctrl+O expand tools · Ctrl+V attach image · Ctrl+Y copy reply\n\
         @ inserts a file, plugin, or skill · / opens the command palette · /model picks models",
    );
    text
}
