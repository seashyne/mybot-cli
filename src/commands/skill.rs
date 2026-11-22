use crate::cli::{Cli, SkillCommand};
use crate::error::{AppError, Result};

use super::working_dir;

pub(super) fn skill_command(cli: &Cli, command: &SkillCommand) -> Result<()> {
    let cwd = working_dir(cli)?;

    match command {
        SkillCommand::List => {
            let skills = crate::skills::SkillSet::discover(&cwd);
            if skills.is_empty() {
                println!("no skills found");
                println!();
                println!("add one with `mybot skill new <name>` or drop a");
                println!("SKILL.md into ~/.mybot/skills/<name>/");
                return Ok(());
            }
            println!("{} skill(s) for {}", skills.len(), cwd.display());
            for skill in &skills.skills {
                println!(
                    "  {:<20} {:<12} {}",
                    skill.name,
                    skill.source.label(),
                    skill.description
                );
            }
        }
        SkillCommand::Show { name } => {
            let skills = crate::skills::SkillSet::discover(&cwd);
            let skill = skills.get(name).ok_or_else(|| {
                AppError::usage(format!(
                    "unknown skill '{name}' (available: {})",
                    skills.names().join(", ")
                ))
            })?;
            println!("{}", skill.body.trim_end());
        }
        SkillCommand::New { name, scope } => {
            if !crate::skills::is_valid_name(name) {
                return Err(AppError::usage(
                    "skill names use lowercase letters, digits, '-' and '_'",
                ));
            }
            let root = if scope == "user" {
                let home = dirs::home_dir()
                    .ok_or_else(|| AppError::runtime("cannot find the home directory"))?;
                home.join(".mybot/skills")
            } else {
                cwd.join(".mybot/skills")
            };
            let dir = root.join(name);
            let path = dir.join("SKILL.md");
            if path.exists() {
                return Err(AppError::usage(format!(
                    "{} already exists",
                    path.display()
                )));
            }
            std::fs::create_dir_all(&dir)?;
            let template = format!(
                "---\nname: {name}\ndescription: Describe when the model should use this skill\n---\n\n# {name}\n\n1. First step\n2. Second step\n\n## Rules\n\n- Keep the instructions concrete and short.\n"
            );
            std::fs::write(&path, template)?;
            println!("created {}", path.display());
        }
    }
    Ok(())
}
