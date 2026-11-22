use clap::Parser;

use mybot::cli::Cli;
use mybot::error::AppError;

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mybot: {error}");
            std::process::ExitCode::from(error.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> mybot::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::runtime(format!("cannot start async runtime: {error}")))?;
    runtime.block_on(mybot::commands::dispatch(cli))
}
