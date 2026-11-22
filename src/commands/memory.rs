use crate::cli::{Cli, MemoryCommand};
use crate::error::{AppError, Result};

use super::working_dir;

pub(super) fn memory_command(cli: &Cli, command: &MemoryCommand) -> Result<()> {
    let cwd = working_dir(cli)?;
    let mut store = crate::memory::MemoryStore::load(&cwd);

    match command {
        MemoryCommand::List => {
            print!("{}", store.render_index());
            println!("\nroot: {}", store.root.display());
        }
        MemoryCommand::Show { name } => {
            let entry = store
                .get(name)
                .ok_or_else(|| AppError::usage(format!("unknown memory '{name}'")))?;
            println!("# {} ({})", entry.title, entry.kind.as_str());
            println!("{}\n", entry.description);
            println!("{}", entry.body);
        }
        MemoryCommand::Delete { name } => {
            if store.delete(name)? {
                println!("deleted memory '{}'", crate::memory::slugify(name));
            } else {
                return Err(AppError::usage(format!("unknown memory '{name}'")));
            }
        }
        MemoryCommand::Path => println!("{}", store.root.display()),
    }
    Ok(())
}
