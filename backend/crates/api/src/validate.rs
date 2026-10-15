//! Validation et normalisation des entrées utilisateur.

use crate::error::AppError;

pub const PASSWORD_MIN: usize = 10;
pub const PASSWORD_MAX: usize = 128;

/// Normalise (trim + minuscules) et vérifie une adresse email.
pub fn email(raw: &str) -> Result<String, AppError> {
    let email = raw.trim().to_lowercase();
    let valid = (3..=254).contains(&email.len())
        && !email.chars().any(char::is_whitespace)
        && matches!(email.split_once('@'), Some((local, domain))
            if !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
               && !domain.ends_with('.') && !domain.contains('@'));
    if valid {
        Ok(email)
    } else {
        Err(AppError::BadRequest("adresse email invalide".into()))
    }
}

pub fn password(raw: &str) -> Result<(), AppError> {
    let len = raw.chars().count();
    if (PASSWORD_MIN..=PASSWORD_MAX).contains(&len) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "le mot de passe doit contenir entre {PASSWORD_MIN} et {PASSWORD_MAX} caractères"
        )))
    }
}

pub fn display_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    let len = name.chars().count();
    if (1..=80).contains(&len) && !name.chars().any(char::is_control) {
        Ok(name.to_owned())
    } else {
        Err(AppError::BadRequest(
            "le nom doit contenir entre 1 et 80 caractères".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_normalized() {
        assert_eq!(email("  Ana@Example.COM ").unwrap(), "ana@example.com");
    }

    #[test]
    fn invalid_emails_are_rejected() {
        for bad in [
            "",
            "ana",
            "ana@",
            "@x.com",
            "ana@localhost",
            "a b@x.com",
            "a@b@c.com",
            "a@.com",
        ] {
            assert!(email(bad).is_err(), "{bad} devrait être refusé");
        }
    }

    #[test]
    fn password_length_bounds() {
        assert!(password("court").is_err());
        assert!(password("assez-long-1").is_ok());
        assert!(password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn display_name_is_trimmed() {
        assert_eq!(display_name("  Ana ").unwrap(), "Ana");
        assert!(display_name("   ").is_err());
        assert!(display_name("a\u{0007}").is_err());
    }
}
