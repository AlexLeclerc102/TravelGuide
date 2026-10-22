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

/// Texte libre borné (sans caractères de contrôle hormis les sauts de ligne).
fn bounded_text(raw: &str, min: usize, max: usize, what: &str) -> Result<String, AppError> {
    let v = raw.trim();
    let len = v.chars().count();
    let clean = !v.chars().any(|c| c.is_control() && c != '\n' && c != '\t');
    if (min..=max).contains(&len) && clean {
        Ok(v.to_owned())
    } else if min > 0 {
        Err(AppError::BadRequest(format!(
            "{what} doit contenir entre {min} et {max} caractères"
        )))
    } else {
        Err(AppError::BadRequest(format!(
            "{what} ne doit pas dépasser {max} caractères"
        )))
    }
}

pub fn poi_name(raw: &str) -> Result<String, AppError> {
    let name = bounded_text(raw, 1, 200, "le nom du lieu")?;
    if name.contains('\n') {
        return Err(AppError::BadRequest(
            "le nom du lieu tient sur une ligne".into(),
        ));
    }
    Ok(name)
}

pub fn address(raw: &str) -> Result<String, AppError> {
    bounded_text(raw, 0, 500, "l'adresse")
}

pub fn review_comment(raw: &str) -> Result<String, AppError> {
    bounded_text(raw, 0, 2000, "le commentaire")
}

/// Note d'un lieu (au moins un caractère).
pub fn note(raw: &str) -> Result<String, AppError> {
    bounded_text(raw, 1, 4000, "la note")
}

/// Première note facultative, à l'ajout d'un lieu (vide : aucune).
pub fn notes(raw: &str) -> Result<String, AppError> {
    bounded_text(raw, 0, 4000, "la note")
}

/// Coordonnées WGS84 finies et dans les bornes.
pub fn coordinates(lat: f64, lon: f64) -> Result<(), AppError> {
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Ok(())
    } else {
        Err(AppError::BadRequest("coordonnées invalides".into()))
    }
}

/// Durée de visite en minutes (0 à 12 h).
pub fn duration_min(v: i32) -> Result<i32, AppError> {
    if (0..=720).contains(&v) {
        Ok(v)
    } else {
        Err(AppError::BadRequest(
            "la durée doit être comprise entre 0 et 720 minutes".into(),
        ))
    }
}

/// Site web : lien http(s) uniquement (pas de `javascript:`), vide => aucun.
pub fn website(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let ok = (v.starts_with("https://") || v.starts_with("http://"))
        && v.len() <= 500
        && !v.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(Some(v.to_owned()))
    } else {
        Err(AppError::BadRequest(
            "le site web doit être un lien http(s) valide".into(),
        ))
    }
}

/// Lien ajouté à un lieu : https uniquement (le contenu est partagé avec tout le groupe).
pub fn link_url(raw: &str) -> Result<String, AppError> {
    let v = raw.trim();
    let ok = v.starts_with("https://")
        && v.len() > "https://".len()
        && v.len() <= 500
        && !v.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(v.to_owned())
    } else {
        Err(AppError::BadRequest(
            "le lien doit être une adresse https valide".into(),
        ))
    }
}

/// Page d'où vient une information (http(s)) ; vide => aucune.
pub fn source_url(raw: Option<&str>) -> Result<Option<String>, AppError> {
    website(raw)
        .map_err(|_| AppError::BadRequest("la source doit être un lien http(s) valide".into()))
}

pub fn link_title(raw: &str) -> Result<String, AppError> {
    let title = bounded_text(raw, 1, 200, "le titre du lien")?;
    if title.contains('\n') {
        return Err(AppError::BadRequest(
            "le titre du lien tient sur une ligne".into(),
        ));
    }
    Ok(title)
}

/// Téléphone d'un lieu : chiffres et séparateurs usuels (`+`, espace, `-`, `.`, `/`,
/// parenthèses) ; vide => aucun.
pub fn phone(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let ok = v.len() <= 50
        && v.chars().filter(char::is_ascii_digit).count() >= 3
        && v.chars()
            .all(|c| c.is_ascii_digit() || "+ -./()".contains(c));
    if ok {
        Ok(Some(v.to_owned()))
    } else {
        Err(AppError::BadRequest("numéro de téléphone invalide".into()))
    }
}

/// Email de contact d'un lieu ; vide => aucun.
pub fn contact_email(raw: Option<&str>) -> Result<Option<String>, AppError> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => email(v).map(Some),
        None => Ok(None),
    }
}

/// Identifiant Wikidata (`Q` suivi d'un nombre) ; vide => aucun.
pub fn wikidata(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let ok = matches!(v.strip_prefix('Q'), Some(n)
        if (1..=12).contains(&n.len()) && !n.starts_with('0')
           && n.chars().all(|c| c.is_ascii_digit()));
    if ok {
        Ok(Some(v.to_owned()))
    } else {
        Err(AppError::BadRequest("identifiant Wikidata invalide".into()))
    }
}

/// Article Wikipédia au format OSM `langue:Titre` (ex. `es:Volcán Arenal`) ; vide => aucun.
pub fn wikipedia(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let ok = matches!(v.split_once(':'), Some((lang, title))
        if (2..=12).contains(&lang.len())
           && lang.chars().all(|c| c.is_ascii_lowercase() || c == '-')
           && !title.trim().is_empty() && !title.starts_with('/')
           && title.chars().count() <= 250
           && !title.chars().any(char::is_control));
    if ok {
        Ok(Some(v.to_owned()))
    } else {
        Err(AppError::BadRequest(
            "article Wikipédia invalide (format langue:Titre)".into(),
        ))
    }
}

/// Code pays ISO à deux lettres, mis en majuscules ; vide => aucun.
pub fn country_code(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if v.len() == 2 && v.chars().all(|c| c.is_ascii_alphabetic()) {
        Ok(Some(v.to_ascii_uppercase()))
    } else {
        Err(AppError::BadRequest("code pays invalide".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, time};

    #[test]
    fn poi_fields() {
        assert_eq!(poi_name("  Arenal ").unwrap(), "Arenal");
        assert!(poi_name("").is_err());
        assert!(poi_name("a\nb").is_err());
        assert!(poi_name(&"x".repeat(201)).is_err());
        assert_eq!(notes("ligne 1\nligne 2").unwrap(), "ligne 1\nligne 2");
        assert!(notes(&"x".repeat(4001)).is_err());
        assert!(note(" ").is_err());
        assert!(note(&"x".repeat(4000)).is_ok());
        assert!(coordinates(9.9, -84.1).is_ok());
        for (lat, lon) in [
            (91.0, 0.0),
            (0.0, -181.0),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(coordinates(lat, lon).is_err(), "{lat},{lon}");
        }
        assert!(duration_min(0).is_ok() && duration_min(720).is_ok());
        assert!(duration_min(-1).is_err() && duration_min(721).is_err());
    }

    #[test]
    fn website_and_country() {
        assert_eq!(website(Some(" ")).unwrap(), None);
        assert!(website(Some("https://example.cr")).unwrap().is_some());
        for bad in [
            "javascript:alert(1)",
            "ftp://x",
            "https://a b.cr",
            "example.cr",
        ] {
            assert!(website(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(country_code(Some("cr")).unwrap().as_deref(), Some("CR"));
        assert!(country_code(Some("CRI")).is_err());
        assert!(country_code(Some("1A")).is_err());
    }

    #[test]
    fn contact_and_references() {
        assert_eq!(phone(Some(" ")).unwrap(), None);
        assert_eq!(
            phone(Some("+506 2257-1433")).unwrap().as_deref(),
            Some("+506 2257-1433")
        );
        for bad in [
            "12",
            "+506 2257 1433; +506 8888 8888",
            "tel:2257",
            &"1".repeat(51),
        ] {
            assert!(phone(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            contact_email(Some(" Info@Museo.CR ")).unwrap().as_deref(),
            Some("info@museo.cr")
        );
        assert!(contact_email(Some("museo")).is_err());
        assert_eq!(
            wikidata(Some("Q190622")).unwrap().as_deref(),
            Some("Q190622")
        );
        for bad in ["Q", "Q0", "Q01", "190622", "Q12a", "q190622"] {
            assert!(wikidata(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            wikipedia(Some("es:Volcán Arenal")).unwrap().as_deref(),
            Some("es:Volcán Arenal")
        );
        for bad in [
            "Volcán Arenal",
            "es:",
            "https://es.wikipedia.org/wiki/Arenal",
            "ES:Arenal",
        ] {
            assert!(wikipedia(Some(bad)).is_err(), "{bad}");
        }
    }

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
