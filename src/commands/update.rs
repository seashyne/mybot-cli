use crate::cli::UpdateArgs;
use crate::error::Result;

pub(super) async fn update_command(args: &UpdateArgs) -> Result<()> {
    let outcome = crate::update::run(crate::update::UpdateOptions {
        check: args.check,
        force: args.force,
        tag: args.tag.clone(),
    })
    .await?;
    println!("{}", outcome.message());
    Ok(())
}
