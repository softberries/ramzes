mod config;
mod ops;
mod serve;
mod ui;

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::config::{Cli, Command, Config};

fn init_logging() {
    let filter = EnvFilter::try_from_env("RAMZES_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if std::env::var("RAMZES_LOG_FORMAT").is_ok_and(|format| format == "json") {
        builder.json().init();
    } else {
        builder.init();
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Command::Openapi) => {
            println!("{}", ramzes_api::openapi().to_pretty_json()?);
            Ok(())
        }
        Some(Command::Backup { file }) => {
            let config = Config::load(cli.options)?;
            let store = serve::open_store(&config).await?;
            store.backup_to(&file).await?;
            store.close().await;
            eprintln!("backup written to {}", file.display());
            Ok(())
        }
        Some(Command::Serve) | None => serve::run(Config::load(cli.options)?).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ramzes: {e:#}");
            ExitCode::FAILURE
        }
    }
}
