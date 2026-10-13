mod config;
mod ops;
mod serve;
mod ui;

use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;
use ramzes_engine::{Clock, SystemClock};
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

fn now_ms() -> i64 {
    SystemClock.now_ms()
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
        Some(Command::Export { file }) => {
            let config = Config::load(cli.options)?;
            let store = serve::open_store(&config).await?;
            store.migrate().await?;
            let workspace_id = store.ensure_workspace(now_ms()).await?;
            let yaml = ramzes_api::to_yaml(store.read_config(&workspace_id).await?);
            store.close().await;
            match file {
                Some(file) => {
                    std::fs::write(&file, yaml)
                        .with_context(|| format!("could not write {}", file.display()))?;
                    eprintln!("configuration written to {}", file.display());
                }
                None => print!("{yaml}"),
            }
            Ok(())
        }
        Some(Command::Import { file }) => {
            let config = Config::load(cli.options)?;
            let yaml = std::fs::read_to_string(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            let store = serve::open_store(&config).await?;
            store.migrate().await?;
            let now = now_ms();
            let workspace_id = store.ensure_workspace(now).await?;
            let key = serve::secret_key(&config)?;
            let registry = serve::registry(&config);
            let imported =
                ramzes_api::import_yaml(&store, &registry, &key, &workspace_id, &yaml, now).await;
            if imported.is_ok() {
                let snapshot = serde_json::to_string(&store.read_config(&workspace_id).await?)?;
                store
                    .add_revision(
                        &workspace_id,
                        "ramzes import",
                        "imported a configuration",
                        &snapshot,
                        now,
                    )
                    .await?;
            }
            store.close().await;
            imported.map_err(|e| anyhow::anyhow!("the configuration was not imported: {e}"))?;
            eprintln!("configuration imported from {}", file.display());
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
