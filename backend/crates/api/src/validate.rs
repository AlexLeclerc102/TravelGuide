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

pub fn trip_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    let len = name.chars().count();
    if (1..=120).contains(&len) && !name.chars().any(char::is_control) {
        Ok(name.to_owned())
    } else {
        Err(AppError::BadRequest(
            "le nom du voyage doit contenir entre 1 et 120 caractères".into(),
        ))
    }
}

pub fn description(raw: &str) -> Result<String, AppError> {
    let d = raw.trim();
    if d.chars().count() <= 2000 {
        Ok(d.to_owned())
    } else {
        Err(AppError::BadRequest(
            "la description ne doit pas dépasser 2000 caractères".into(),
        ))
    }
}

/// Durée maximale d'un voyage (en jours d'écart entre début et fin).
pub const MAX_TRIP_SPAN_DAYS: i64 = 60;

pub fn trip_dates(start: time::Date, end: time::Date) -> Result<(), AppError> {
    let span = (end - start).whole_days();
    if span < 0 {
        return Err(AppError::BadRequest(
            "la date de fin doit suivre la date de début".into(),
        ));
    }
    if span > MAX_TRIP_SPAN_DAYS {
        return Err(AppError::BadRequest(format!(
            "un voyage ne peut pas dépasser {} jours",
            MAX_TRIP_SPAN_DAYS + 1
        )));
    }
    Ok(())
}

/// Plage horaire quotidienne : la fin doit suivre le début.
pub fn day_window(start: time::Time, end: time::Time) -> Result<(), AppError> {
    if end > start {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "l'heure de fin de journée doit suivre l'heure de début".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, time};

    #[test]
    fn trip_dates_bounds() {
        assert!(trip_dates(date!(2026 - 12 - 01), date!(2026 - 12 - 01)).is_ok());
        assert!(trip_dates(date!(2026 - 12 - 02), date!(2026 - 12 - 01)).is_err());
        assert!(trip_dates(date!(2026 - 12 - 01), date!(2027 - 01 - 30)).is_ok());
        assert!(trip_dates(date!(2026 - 12 - 01), date!(2027 - 01 - 31)).is_err());
    }

    #[test]
    fn day_window_order() {
        assert!(day_window(time!(9:00), time!(19:00)).is_ok());
        assert!(day_window(time!(19:00), time!(9:00)).is_err());
    }

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
