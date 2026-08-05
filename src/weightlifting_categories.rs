//! Kategorie wagowe PZPC / IWF (sezon 2026) — przypisanie z wieku i masy ciała.

use chrono::{Datelike, NaiveDate};

use crate::error::{AppError, AppResult};
use crate::models::club::AthleteProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeGroup {
    Senior,
    U23,
    U20,
    U17,
    U15,
}

impl AgeGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Senior => "senior",
            Self::U23 => "u23",
            Self::U20 => "u20",
            Self::U17 => "u17",
            Self::U15 => "u15",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Senior => "Senior",
            Self::U23 => "U23",
            Self::U20 => "U20",
            Self::U17 => "U17",
            Self::U15 => "U15",
        }
    }

    pub fn from_age(age: u32) -> Self {
        if age < 15 {
            Self::U15
        } else if age < 17 {
            Self::U17
        } else if age < 20 {
            Self::U20
        } else if age < 23 {
            Self::U23
        } else {
            Self::Senior
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AthleteSex {
    Male,
    Female,
}

impl AthleteSex {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "male" | "m" | "mezczyzna" | "mężczyzna" => Some(Self::Male),
            "female" | "f" | "kobieta" | "k" => Some(Self::Female),
            _ => None,
        }
    }

    fn sex_key(self, group: AgeGroup) -> &'static str {
        match (group, self) {
            (AgeGroup::U15 | AgeGroup::U17, Self::Male) => "boys",
            (AgeGroup::U15 | AgeGroup::U17, Self::Female) => "girls",
            (_, Self::Male) => "men",
            (_, Self::Female) => "women",
        }
    }

    fn label(self, group: AgeGroup) -> &'static str {
        match (group, self) {
            (AgeGroup::U15 | AgeGroup::U17, Self::Male) => "Chł",
            (AgeGroup::U15 | AgeGroup::U17, Self::Female) => "Dz",
            (_, Self::Male) => "M",
            (_, Self::Female) => "K",
        }
    }
}

/// Kategorie wagowe 2026 (limity w kg; `+N` = open powyżej N).
fn weight_classes(group: AgeGroup, sex: AthleteSex) -> &'static [&'static str] {
    match (group, sex.sex_key(group)) {
        (AgeGroup::Senior | AgeGroup::U23 | AgeGroup::U20, "men") => {
            &["60", "65", "70", "75", "85", "95", "110", "+110"]
        }
        (AgeGroup::Senior | AgeGroup::U23 | AgeGroup::U20, "women") => {
            &["49", "53", "57", "61", "69", "77", "86", "+86"]
        }
        (AgeGroup::U17, "boys") => &["55", "60", "65", "70", "75", "85", "95", "+95"],
        (AgeGroup::U17, "girls") => &["45", "49", "53", "57", "61", "69", "77", "+77"],
        (AgeGroup::U15, "boys") => &["51", "55", "60", "65", "70", "75", "85", "+85"],
        (AgeGroup::U15, "girls") => &["41", "45", "49", "53", "57", "61", "69", "+69"],
        _ => &[],
    }
}

pub fn age_from_birth_date(birth_date: &str, on: NaiveDate) -> AppResult<u32> {
    let birth = NaiveDate::parse_from_str(birth_date.trim(), "%Y-%m-%d").map_err(|_| {
        AppError::BadRequest("Nieprawidłowa data urodzenia w profilu (oczekiwano YYYY-MM-DD).".into())
    })?;
    let mut age = on.year() - birth.year();
    if (on.month(), on.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    if age < 0 || age > 120 {
        return Err(AppError::BadRequest("Nieprawidłowy wiek w profilu.".into()));
    }
    Ok(age as u32)
}

pub fn pick_weight_class(bodyweight_kg: f64, classes: &[&str]) -> AppResult<String> {
    if !bodyweight_kg.is_finite() || bodyweight_kg <= 0.0 {
        return Err(AppError::BadRequest("Podaj poprawną masę ciała (kg).".into()));
    }
    if classes.is_empty() {
        return Err(AppError::BadRequest("Brak tabeli kategorii wagowych.".into()));
    }
    for class in classes {
        if let Some(rest) = class.strip_prefix('+') {
            let _limit: f64 = rest.parse().map_err(|_| {
                AppError::BadRequest(format!("Nieprawidłowa kategoria open: {class}"))
            })?;
            return Ok((*class).to_string());
        }
        let limit: f64 = class
            .parse()
            .map_err(|_| AppError::BadRequest(format!("Nieprawidłowa kategoria: {class}")))?;
        if bodyweight_kg <= limit {
            return Ok((*class).to_string());
        }
    }
    Ok((*classes.last().unwrap()).to_string())
}

/// Format: `U20 M 75`, `Senior K +86`, `U15 Chł 55`.
pub fn format_category(group: AgeGroup, sex: AthleteSex, weight_class: &str) -> String {
    format!("{} {} {}", group.label(), sex.label(group), weight_class)
}

pub fn resolve_category(
    birth_date: &str,
    sex_raw: &str,
    bodyweight_kg: f64,
    on: NaiveDate,
) -> AppResult<String> {
    let age = age_from_birth_date(birth_date, on)?;
    let sex = AthleteSex::parse(sex_raw).ok_or_else(|| {
        AppError::BadRequest("W profilu brakuje płci (male/female) — uzupełnij profil.".into())
    })?;
    let group = AgeGroup::from_age(age);
    let classes = weight_classes(group, sex);
    let weight_class = pick_weight_class(bodyweight_kg, classes)?;
    Ok(format_category(group, sex, &weight_class))
}

pub fn resolve_category_from_profile(
    profile: &AthleteProfile,
    bodyweight_kg: f64,
    on: NaiveDate,
) -> AppResult<String> {
    let birth = profile.birth_date.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let sex = profile.sex.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match (birth, sex) {
        (Some(b), Some(s)) => resolve_category(b, s, bodyweight_kg, on),
        _ => Err(AppError::BadRequest(
            "Uzupełnij w profilu datę urodzenia i płeć — kategoria wagowa wylicza się automatycznie."
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_groups() {
        assert_eq!(AgeGroup::from_age(14), AgeGroup::U15);
        assert_eq!(AgeGroup::from_age(15), AgeGroup::U17);
        assert_eq!(AgeGroup::from_age(16), AgeGroup::U17);
        assert_eq!(AgeGroup::from_age(19), AgeGroup::U20);
        assert_eq!(AgeGroup::from_age(22), AgeGroup::U23);
        assert_eq!(AgeGroup::from_age(23), AgeGroup::Senior);
    }

    #[test]
    fn weight_pick() {
        let men = weight_classes(AgeGroup::Senior, AthleteSex::Male);
        assert_eq!(pick_weight_class(59.9, men).unwrap(), "60");
        assert_eq!(pick_weight_class(60.0, men).unwrap(), "60");
        assert_eq!(pick_weight_class(60.1, men).unwrap(), "65");
        assert_eq!(pick_weight_class(110.0, men).unwrap(), "110");
        assert_eq!(pick_weight_class(110.1, men).unwrap(), "+110");
    }

    #[test]
    fn resolve_u20_men() {
        let on = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        // ~18 lat
        let cat = resolve_category("2008-01-01", "male", 74.5, on).unwrap();
        assert_eq!(cat, "U20 M 75");
    }

    #[test]
    fn resolve_u15_girls() {
        let on = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let cat = resolve_category("2012-06-01", "female", 48.2, on).unwrap();
        assert_eq!(cat, "U15 Dz 49");
    }
}
