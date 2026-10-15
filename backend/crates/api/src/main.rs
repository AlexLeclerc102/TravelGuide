use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::mail::{LogMailer, Mailer, SmtpMailer};
use api::places::nominatim::Nominatim;
use api::places::PlaceService;
use api::routing::valhalla::Valhalla;
use api::routing::RoutingService;
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

    if config.nominatim_contact.is_none() {
        tracing::warn!("NOMINATIM_CONTACT absent : renseignez un contact pour respecter la politique d'usage de Nominatim");
    }
    let places = Arc::new(PlaceService::new(Nominatim::new(
        &config.nominatim_url,
        &config.user_agent(),
    )?));

    let routing = Arc::new(match &config.valhalla_url {
        Some(url) => RoutingService::new(Valhalla::new(url, &config.user_agent())?),
        None => {
            tracing::warn!("VALHALLA_URL absent : les trajets seront estimés à vol d'oiseau");
            RoutingService::straight_line_only()
        }
    });

    let addr = config.bind_addr;
    let app = api::build_app(db, config, mailer, places, routing);
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
