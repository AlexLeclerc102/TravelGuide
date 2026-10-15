//! Interprétation des horaires d'ouverture OSM (`opening_hours`) sur les dates du voyage.
//!
//! Les horaires sont exprimés en heure locale du lieu, comme les dates du voyage :
//! aucune conversion de fuseau n'est nécessaire. Les jours fériés (`PH`) sont pris
//! en compte quand le pays du lieu est connu.

use std::str::FromStr;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use opening_hours::localization::Country;
use opening_hours::{Context, OpeningHours, RuleKind};
use serde::Serialize;
use time::Date;
use utoipa::ToSchema;

use crate::error::AppError;

pub const MAX_LEN: usize = 500;

/// État de la valeur `opening_hours` d'un POI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HoursStatus {
    /// Pas d'horaires connus.
    None,
    Valid,
    /// Syntaxe non reconnue : ignorée par le planificateur, à corriger.
    Invalid,
}

/// Plage horaire d'une journée (`HH:MM`, fin à `24:00` pour minuit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Interval {
    pub start: String,
    pub end: String,
    /// Ouverture incertaine (`unknown` dans la syntaxe OSM, ex. « sur rendez-vous »).
    pub unknown: bool,
    /// Commentaire éventuel de la règle OSM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Horaires interprétés pour une date du voyage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DaySchedule {
    #[schema(value_type = String, format = Date)]
    pub date: Date,
    /// Vide : fermé toute la journée.
    pub intervals: Vec<Interval>,
    /// Jour férié dans le pays du lieu.
    pub holiday: bool,
}

/// Normalise une valeur saisie : espaces retirés, vide => `None`, longueur bornée.
/// Une syntaxe invalide est acceptée (voir [`HoursStatus::Invalid`]).
pub fn normalize(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(v) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > MAX_LEN || v.chars().any(char::is_control) {
        return Err(AppError::BadRequest(format!(
            "les horaires ne doivent pas dépasser {MAX_LEN} caractères"
        )));
    }
    Ok(Some(v.to_owned()))
}

/// Résultat de l'analyse syntaxique.
pub enum Parsed {
    None,
    Valid(Box<OpeningHours>),
    Invalid(String),
}

pub fn parse(raw: Option<&str>, country_code: Option<&str>) -> Parsed {
    let Some(raw) = raw else {
        return Parsed::None;
    };
    match OpeningHours::from_str(raw) {
        Ok(oh) => {
            let ctx = match country_code.and_then(|c| Country::from_str(c).ok()) {
                Some(country) => Context::default().with_holidays(country.holidays()),
                None => Context::default(),
            };
            Parsed::Valid(Box::new(oh.with_context(ctx)))
        }
        Err(e) => Parsed::Invalid(short_error(&e.to_string())),
    }
}

/// Résume l'erreur du parseur (très verbeuse) en une position lisible.
fn short_error(raw: &str) -> String {
    // Format : « --> ligne:colonne » suivi d'un extrait et de la liste des règles attendues.
    let column = raw
        .split("-->")
        .nth(1)
        .and_then(|rest| rest.trim().split(|c: char| c.is_whitespace()).next())
        .and_then(|pos| pos.split(':').nth(1))
        .and_then(|col| col.parse::<usize>().ok());
    match column {
        Some(col) => format!("erreur de syntaxe vers le caractère {col}"),
        None => "erreur de syntaxe".to_owned(),
    }
}

impl Parsed {
    pub fn status(&self) -> HoursStatus {
        match self {
            Parsed::None => HoursStatus::None,
            Parsed::Valid(_) => HoursStatus::Valid,
            Parsed::Invalid(_) => HoursStatus::Invalid,
        }
    }
}

fn to_naive(d: Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), u8::from(d.month()).into(), d.day().into())
        .expect("date valide")
}

fn hhmm(dt: NaiveDateTime, day: NaiveDate) -> String {
    if dt.date() > day {
        "24:00".to_owned()
    } else {
        dt.format("%H:%M").to_string()
    }
}

/// Horaires d'une journée (heure locale).
pub fn day_schedule(oh: &OpeningHours, date: Date) -> DaySchedule {
    let day = to_naive(date);
    let from = day.and_time(NaiveTime::MIN);
    let to = from + chrono::Duration::days(1);
    let intervals = oh
        .iter_range(from, to)
        .filter(|r| matches!(r.kind, RuleKind::Open | RuleKind::Unknown))
        .map(|r| Interval {
            start: hhmm(r.range.start, day),
            end: hhmm(r.range.end, day),
            unknown: r.kind == RuleKind::Unknown,
            comment: (!r.comment.is_empty()).then(|| r.comment.to_string()),
        })
        .collect();
    let holiday = oh.get_context().holidays.get_public().contains(day);
    DaySchedule {
        date,
        intervals,
        holiday,
    }
}

/// Horaires de chaque date de `start` à `end` incluses.
pub fn schedule(oh: &OpeningHours, start: Date, end: Date) -> Vec<DaySchedule> {
    let mut out = Vec::new();
    let mut d = start;
    while d <= end {
        out.push(day_schedule(oh, d));
        match d.next_day() {
            Some(n) => d = n,
            None => break,
        }
    }
    out
}

/// Dates du voyage où le lieu est fermé toute la journée.
pub fn closed_dates(oh: &OpeningHours, start: Date, end: Date) -> Vec<Date> {
    schedule(oh, start, end)
        .into_iter()
        .filter(|d| d.intervals.is_empty())
        .map(|d| d.date)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn valid(raw: &str, cc: Option<&str>) -> OpeningHours {
        match parse(Some(raw), cc) {
            Parsed::Valid(oh) => *oh,
            Parsed::Invalid(e) => panic!("{raw} : {e}"),
            Parsed::None => unreachable!(),
        }
    }

    #[test]
    fn normalize_trims_and_bounds() {
        assert_eq!(normalize(Some("  ")).unwrap(), None);
        assert_eq!(normalize(None).unwrap(), None);
        assert_eq!(
            normalize(Some(" Mo-Fr 09:00-17:00 ")).unwrap().as_deref(),
            Some("Mo-Fr 09:00-17:00")
        );
        assert!(normalize(Some(&"x".repeat(501))).is_err());
        // Une syntaxe invalide reste acceptée.
        assert!(normalize(Some("ouvert le matin")).unwrap().is_some());
    }

    #[test]
    fn syntax_errors_are_summarized() {
        let Parsed::Invalid(e) = parse(Some("Mo-Fr 09:00-17:00; ouvert"), None) else {
            panic!("devrait être invalide");
        };
        assert_eq!(e, "erreur de syntaxe vers le caractère 20");
        assert_eq!(short_error("autre chose"), "erreur de syntaxe");
    }

    #[test]
    fn status_reflects_syntax() {
        assert_eq!(parse(None, None).status(), HoursStatus::None);
        assert_eq!(parse(Some("24/7"), None).status(), HoursStatus::Valid);
        assert_eq!(
            parse(Some("ouvert le matin"), None).status(),
            HoursStatus::Invalid
        );
    }

    #[test]
    fn museum_schedule_over_a_week() {
        // Musée national de Costa Rica (valeur réelle OSM). Le 2026-12-01 est un mardi.
        let oh = valid("Tu-Sa 08:30-16:30; Su 09:00-16:30", None);
        let week = schedule(&oh, date!(2026 - 12 - 01), date!(2026 - 12 - 07));
        assert_eq!(week.len(), 7);
        assert_eq!(week[0].intervals[0].start, "08:30");
        assert_eq!(week[0].intervals[0].end, "16:30");
        assert_eq!(week[5].intervals[0].start, "09:00"); // dimanche
        assert!(week[6].intervals.is_empty()); // lundi : fermé
        assert_eq!(
            closed_dates(&oh, date!(2026 - 12 - 01), date!(2026 - 12 - 07)),
            vec![date!(2026 - 12 - 07)]
        );
    }

    #[test]
    fn split_hours_and_midnight() {
        let oh = valid("Mo-Su 12:00-15:00,19:00-24:00", None);
        let d = day_schedule(&oh, date!(2026 - 12 - 01));
        let spans: Vec<_> = d
            .intervals
            .iter()
            .map(|i| (i.start.as_str(), i.end.as_str()))
            .collect();
        assert_eq!(spans, vec![("12:00", "15:00"), ("19:00", "24:00")]);
    }

    #[test]
    fn unknown_rules_are_flagged() {
        let oh = valid("Mo-Fr 10:00-12:00 unknown \"sur rendez-vous\"", None);
        let d = day_schedule(&oh, date!(2026 - 12 - 01));
        assert!(d.intervals[0].unknown);
        assert_eq!(d.intervals[0].comment.as_deref(), Some("sur rendez-vous"));
    }

    #[test]
    fn public_holidays_use_country() {
        // 25 décembre : férié au Costa Rica.
        let raw = "Mo-Su 09:00-17:00; PH off";
        let christmas = date!(2026 - 12 - 25);
        let cr = day_schedule(&valid(raw, Some("CR")), christmas);
        assert!(cr.holiday);
        assert!(cr.intervals.is_empty());
        // Pays inconnu : pas de jours fériés appliqués.
        let unknown = day_schedule(&valid(raw, None), christmas);
        assert!(!unknown.holiday);
        assert_eq!(unknown.intervals.len(), 1);
    }
}
