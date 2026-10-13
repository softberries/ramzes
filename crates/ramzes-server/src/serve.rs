use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use ramzes_api::{AppState, SecretKey};
use ramzes_connectors::{HttpOptions, Registry};
use ramzes_core::RetryPolicy;
use ramzes_engine::{Clock, Engine, EngineConfig, SystemClock};
use ramzes_ingest::{IngestLimits, IngestState};
use ramzes_store::Store;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::ops::Ops;
use crate::ui;

/// Longest a single request to a channel may take.
const CONNECTOR_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the workers look for work when nothing wakes them.
const POLL: Duration = Duration::from_secs(1);
const RETENTION_EVERY: Duration = Duration::from_secs(3600);
/// How long open connections (such as live streams) get to finish after a shutdown signal.
const HTTP_GRACE: Duration = Duration::from_secs(3);
/// How long in-flight deliveries get to finish after a shutdown signal.
const ENGINE_GRACE: Duration = Duration::from_secs(20);

/// Opens the database named in the config, creating the data directory for SQLite.
pub async fn open_store(config: &Config) -> anyhow::Result<Store> {
    if config.database_url.starts_with("sqlite:") {
        std::fs::create_dir_all(&config.data_dir).with_context(|| {
            format!(
                "could not create the data directory {}",
                config.data_dir.display()
            )
        })?;
    }
    Store::connect(&config.database_url)
        .await
        .context("could not open the database")
}

/// The connectors shipped with ramzes, set up the way the instance is configured.
pub fn registry(config: &Config) -> Registry {
    Registry::with_options(HttpOptions {
        timeout: CONNECTOR_TIMEOUT,
        block_private: config.block_private_addresses,
    })
}

pub fn secret_key(config: &Config) -> anyhow::Result<SecretKey> {
    match &config.secret_key {
        Some(text) => SecretKey::from_base64(text).map_err(|e| anyhow::anyhow!(e)),
        None => {
            std::fs::create_dir_all(&config.data_dir).with_context(|| {
                format!(
                    "could not create the data directory {}",
                    config.data_dir.display()
                )
            })?;
            SecretKey::load_or_create(&config.data_dir)
                .context("could not load or create the secret key")
        }
    }
}

async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = interrupt => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(_) => {
                let _ = interrupt.await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = interrupt.await;
    }
}

pub async fn run(config: Config) -> anyhow::Result<()> {
    let store = open_store(&config).await?;
    store.migrate().await?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let workspace_id = store.ensure_workspace(clock.now_ms()).await?;
    let key = Arc::new(secret_key(&config)?);
    let registry = registry(&config);
    ramzes_api::refresh_custom_connectors(&store, &registry, &workspace_id).await?;

    let engine = Engine::new(EngineConfig {
        store: store.clone(),
        registry: registry.clone(),
        opener: key.clone(),
        clock: clock.clone(),
        policy: RetryPolicy::default(),
        public_url: config.public_url.clone(),
        lease_ms: config.lease_ms,
        jitter: Engine::random_jitter(),
    });
    let ingest = IngestState::new(
        store.clone(),
        engine.clone(),
        IngestLimits::default(),
        clock.clone(),
        key.clone(),
    );
    let ops = Ops::new(store.clone(), ingest.clone());
    let (api, _) = ramzes_api::router(AppState {
        store: store.clone(),
        engine: engine.clone(),
        registry,
        key,
        clock,
        workspace_id: workspace_id.clone(),
        secure_cookies: config.secure_cookies,
        login_guard: Default::default(),
    });
    let app = api
        .merge(ramzes_ingest::router(ingest))
        .merge(ops.clone().router())
        .fallback(ui::serve);

    let shutdown = CancellationToken::new();
    let workers = tokio::spawn(engine.clone().run(shutdown.clone(), POLL));
    tokio::spawn(ops.watch(engine.clone()));
    let retention = {
        let (engine, shutdown) = (engine.clone(), shutdown.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(RETENTION_EVERY);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _ = tick.tick() => {}
                }
                match engine.retention_once(&workspace_id).await {
                    Ok(0) => {}
                    Ok(deleted) => tracing::info!(deleted, "removed events past their retention"),
                    Err(e) => tracing::error!(error = %e, "retention pass failed"),
                }
            }
        })
    };

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("could not listen on {}", config.listen))?;
    tracing::info!(address = %config.listen, database = store.backend(), "ramzes is listening");
    if store.count_users().await? == 0 {
        tracing::info!(
            "First run: open http://{}/setup to create the admin account",
            config.listen
        );
    }

    let stop = shutdown.clone();
    let server =
        axum::serve(listener, app).with_graceful_shutdown(async move { stop.cancelled().await });
    tokio::select! {
        result = server => result.context("the HTTP server failed")?,
        () = async {
            shutdown_signal().await;
            tracing::info!("shutting down");
            shutdown.cancel();
            // Live streams never end on their own; do not wait for them for long.
            tokio::time::sleep(HTTP_GRACE).await;
        } => {}
    }
    shutdown.cancel();
    if tokio::time::timeout(ENGINE_GRACE, workers).await.is_err() {
        tracing::warn!(
            "deliveries still in flight were abandoned; they will be retried on next start"
        );
    }
    retention.abort();
    store.close().await;
    Ok(())
}
