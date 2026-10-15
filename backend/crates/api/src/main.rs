use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::mail::{LogMailer, Mailer, SmtpMailer};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,api=debug".into()),
        )
        .init();

    let config = api::config::Config::from_env()?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .context("connexion à PostgreSQL impossible")?;
    api::MIGRATOR
        .run(&db)
        .await
        .context("échec des migrations")?;

    let mailer: Arc<dyn Mailer> = match &config.smtp_url {
        Some(url) => Arc::new(SmtpMailer::new(url, &config.mail_from)?),
        None => {
            tracing::warn!("SMTP_URL absent : les emails seront seulement journalisés");
            Arc::new(LogMailer)
        }
    };

    let addr = config.bind_addr;
    let app = api::build_app(db, config, mailer);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "API démarrée");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
