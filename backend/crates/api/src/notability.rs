//! Notoriété des lieux importés : nombre de langues de leur article Wikipédia et autres signaux
//! d'intérêt (guide Wikivoyage, patrimoine mondial, superficie, consultations), stockés par
//! élément Wikidata (`wikidata_notability`) et rafraîchis en tâche de fond.
//!
//! Les suggestions autour d'un voyage peuvent interroger Wikidata à la volée (quelques
//! centaines de lieux) ; la découverte du pays et les régions classent tout l'extrait, ce qui
//! n'est possible qu'avec la notoriété déjà en base.

use std::time::Duration;

use sqlx::PgPool;

use crate::wiki::{WikiError, WikiService};

/// Âge au-delà duquel une notoriété est redemandée.
const MAX_AGE_DAYS: i32 = 30;
/// Éléments par requête Wikidata (limite de `wbgetentities`).
const CHUNK: usize = 50;
/// Pause entre deux requêtes : Wikidata demande de ne pas les enchaîner.
const PAUSE: Duration = Duration::from_millis(250);
/// Pause entre deux demandes de consultations (une par élément) : au-delà de quelques requêtes
/// par seconde, l'API répond « 429 Too Many Requests ».
const VIEWS_PAUSE: Duration = Duration::from_millis(250);

/// Demande à Wikidata (et aux statistiques de Wikipédia) les signaux des éléments inconnus ou
/// trop anciens ; renvoie le nombre
/// d'éléments mis à jour. S'arrête à la première panne (reprise au prochain passage).
pub async fn refresh(db: &PgPool, wiki: &WikiService) -> anyhow::Result<usize> {
    let qids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT o.wikidata FROM osm_places o
         LEFT JOIN wikidata_notability n ON n.qid = o.wikidata
         WHERE o.wikidata ~ '^Q[0-9]+$'
           AND (n.qid IS NULL OR n.fetched_at < now() - make_interval(days => $1))
         ORDER BY 1",
    )
    .bind(MAX_AGE_DAYS)
    .fetch_all(db)
    .await?;
    let mut updated = 0;
    for (i, chunk) in qids.chunks(CHUNK).enumerate() {
        if i > 0 {
            tokio::time::sleep(PAUSE).await;
        }
        let mut found = match wiki.fetch_signals(chunk).await {
            Ok(found) => found,
            Err(WikiError::Unavailable(e)) => {
                tracing::warn!(error = ?e, updated, "notoriété Wikidata interrompue");
                break;
            }
        };
        // Absent de la réponse : l'élément n'existe pas (aucun signal).
        let signals: Vec<_> = chunk
            .iter()
            .map(|q| found.remove(q).unwrap_or_default())
            .collect();
        let mut views = Vec::with_capacity(chunk.len());
        for s in &signals {
            if !s.articles.is_empty() {
                tokio::time::sleep(VIEWS_PAUSE).await;
            }
            match wiki.fetch_monthly_views(&s.articles).await {
                Ok(n) => views.push(n.min(i32::MAX as u32) as i32),
                Err(WikiError::Unavailable(e)) => {
                    tracing::warn!(error = ?e, updated, "consultations Wikipédia interrompues");
                    return Ok(updated);
                }
            }
        }
        sqlx::query(
            "INSERT INTO wikidata_notability
                 (qid, languages, wikivoyage, world_heritage, area_km2, monthly_views, fetched_at)
             SELECT q, l, v, w, a, m, now()
             FROM UNNEST($1::text[], $2::int4[], $3::bool[], $4::bool[], $5::float8[], $6::int4[])
                  AS u(q, l, v, w, a, m)
             ON CONFLICT (qid) DO UPDATE SET languages = EXCLUDED.languages,
                 wikivoyage = EXCLUDED.wikivoyage, world_heritage = EXCLUDED.world_heritage,
                 area_km2 = EXCLUDED.area_km2, monthly_views = EXCLUDED.monthly_views,
                 fetched_at = EXCLUDED.fetched_at",
        )
        .bind(chunk)
        .bind(
            signals
                .iter()
                .map(|s| s.languages as i32)
                .collect::<Vec<_>>(),
        )
        .bind(signals.iter().map(|s| s.wikivoyage).collect::<Vec<_>>())
        .bind(signals.iter().map(|s| s.world_heritage).collect::<Vec<_>>())
        .bind(signals.iter().map(|s| s.area_km2).collect::<Vec<_>>())
        .bind(&views)
        .execute(db)
        .await?;
        updated += chunk.len();
    }
    Ok(updated)
}
