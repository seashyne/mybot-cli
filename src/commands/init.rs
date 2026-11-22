use std::path::Path;

use crate::error::Result;

pub(super) fn init_command(directory: Option<&Path>) -> Result<()> {
    let dir = match directory {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("AGENTS.md");
    if path.exists() {
        println!("{} already exists", path.display());
        return Ok(());
    }
    let template = r#"# Project instructions

Describe how agents should work in this repository.

## Build and test

- Build:
- Test:

## Conventions

- 
"#;
    std::fs::write(&path, template)?;
    println!("Created {}", path.display());
    Ok(())
}
