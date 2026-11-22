use crate::error::Result;

pub(super) fn version_command(json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&crate::version::json())?
        );
    } else {
        println!("{}", crate::version::long());
    }
    Ok(())
}
