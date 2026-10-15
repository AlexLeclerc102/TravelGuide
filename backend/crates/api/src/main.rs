use anyhow::Context;
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

    let addr = config.bind_addr;
    let app = api::build_app(db, config);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "API démarrée");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
