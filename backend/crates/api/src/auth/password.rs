//! Hachage des mots de passe avec Argon2id (paramètres par défaut recommandés).
//!
//! Le calcul est coûteux en CPU : il est exécuté hors du runtime asynchrone.

use std::sync::LazyLock;

use anyhow::anyhow;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

/// Empreinte factice vérifiée lorsque le compte n'existe pas, pour que la durée
/// de réponse ne révèle pas l'existence d'une adresse email.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| hash_sync("mot-de-passe-factice").expect("hachage factice"));

fn hash_sync(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hachage du mot de passe : {e}"))
}

fn verify_sync(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .and_then(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed))
        .is_ok()
}

pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || hash_sync(&password)).await?
}

/// Vérifie un mot de passe. `hash = None` (compte inexistant ou sans mot de passe)
/// effectue tout de même une vérification factice et renvoie `false`.
pub async fn verify(password: String, hash: Option<String>) -> anyhow::Result<bool> {
    Ok(tokio::task::spawn_blocking(move || match hash {
        Some(h) => verify_sync(&password, &h),
        None => {
            let _ = verify_sync(&password, &DUMMY_HASH);
            false
        }
    })
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let h = hash("correct horse battery".into()).await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse battery".into(), Some(h.clone()))
            .await
            .unwrap());
        assert!(!verify("mauvais".into(), Some(h)).await.unwrap());
        assert!(!verify("x".into(), None).await.unwrap());
    }
}
