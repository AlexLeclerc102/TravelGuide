//! Jetons opaques aléatoires (sessions, liens magiques, invitations).
//!
//! Seule l'empreinte SHA-256 du jeton est stockée en base : une fuite de la base
//! ne permet pas de se connecter.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Génère un jeton de 256 bits encodé en base64url, avec son empreinte.
pub fn generate() -> (String, Vec<u8>) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash(&token);
    (token, hash)
}

pub fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_matches() {
        let (a, ha) = generate();
        let (b, _) = generate();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(hash(&a), ha);
    }
}
