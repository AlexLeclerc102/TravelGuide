//! Notoriété des lieux importés : nombre de langues de leur article Wikipédia, stocké par
//! élément Wikidata (`wikidata_notability`) et rafraîchi en tâche de fond.
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

/// Demande à Wikidata la notoriété des éléments inconnus ou trop anciens ; renvoie le nombre
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
        let found = match wiki.fetch_languages(chunk).await {
            Ok(found) => found,
            Err(WikiError::Unavailable(e)) => {
                tracing::warn!(error = ?e, updated, "notoriété Wikidata interrompue");
                break;
            }
        };
        // Absent de la réponse : l'élément n'existe pas (0 langue).
        let counts: Vec<i32> = chunk
            .iter()
            .map(|q| found.get(q).copied().unwrap_or(0) as i32)
            .collect();
        sqlx::query(
            "INSERT INTO wikidata_notability (qid, languages, fetched_at)
             SELECT q, l, now() FROM UNNEST($1::text[], $2::int4[]) AS u(q, l)
             ON CONFLICT (qid) DO UPDATE SET languages = EXCLUDED.languages,
                 fetched_at = EXCLUDED.fetched_at",
        )
        .bind(chunk)
        .bind(&counts)
        .execute(db)
        .await?;
        updated += chunk.len();
    }
    Ok(updated)
}
