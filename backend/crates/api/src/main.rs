use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use api::mail::{LogMailer, Mailer, SmtpMailer};
use api::places::nominatim::Nominatim;
use api::places::PlaceService;
use api::routing::valhalla::Valhalla;
use api::routing::RoutingService;
use api::wiki::{WikiService, Wikimedia};
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

    let wiki = Arc::new(WikiService::new(Wikimedia::new(&config.user_agent())?));

    // Lieux des suggestions : import en tâche de fond, l'API répond pendant ce temps ; puis
    // notoriété Wikidata des lieux, rafraîchie chaque jour.
    match config.osm_extract.clone() {
        Some(path) => {
            let (db, wiki) = (db.clone(), wiki.clone());
            tokio::spawn(async move {
                if let Err(e) = api::osm_import::import_if_changed(&db, &path).await {
                    tracing::warn!(error = ?e, "import des lieux OSM en échec");
                }
                loop {
                    match api::notability::refresh(&db, &wiki).await {
                        Ok(n) => tracing::info!(count = n, "notoriété Wikidata à jour"),
                        Err(e) => tracing::warn!(error = ?e, "notoriété Wikidata en échec"),
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
                }
            });
        }
        None => tracing::warn!("OSM_EXTRACT absent : pas de suggestions de lieux"),
    }

    let addr = config.bind_addr;
    let app = api::build_app(db, config, mailer, places, routing, wiki);
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
