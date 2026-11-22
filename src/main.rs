mod app;
mod db;
mod domain;
mod web;

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use chrono_tz::Tz;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::app::Store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let tz: Tz = match std::env::var("TZ") {
        Ok(name) if !name.is_empty() => name
            .parse()
            .map_err(|e| anyhow::anyhow!("TZ={name:?} is not an IANA time zone: {e}"))?,
        _ => Tz::UTC,
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .context("connecting to Postgres")?;
    sqlx::migrate!()
        .run(&pool)
        .await
        .context("running migrations")?;

    let store = Store { pool, tz };
    tracing::info!("time zone {tz}; today is {}", store.today());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.context("binding address")?;
    tracing::info!("listening on {addr}");

    axum::serve(listener, web::router(store))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
}

/// Resolves on Ctrl+C or SIGTERM (sent by Docker/Dokploy on stop/redeploy).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
