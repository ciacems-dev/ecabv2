//! Lecture des formulaires (multipart, avec fichiers) et conversion des champs.

use std::collections::HashMap;

use axum::extract::Multipart;
use chrono::NaiveDate;
use tower_sessions::Session;

use crate::error::{AppError, AppResult};
use crate::session;

pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

pub struct FormData {
    pub fields: HashMap<String, String>,
    pub files: HashMap<String, Upload>,
}

impl FormData {
    /// Lit tout le formulaire et vérifie le jeton CSRF (`_csrf_token`).
    pub async fn read(session: &Session, mut mp: Multipart) -> AppResult<Self> {
        let mut fields = HashMap::new();
        let mut files = HashMap::new();
        while let Some(field) = mp
            .next_field()
            .await
            .map_err(|_| AppError::BadRequest("Formulaire illisible (fichier trop volumineux ?)."))?
        {
            let name = field.name().unwrap_or_default().to_string();
            if let Some(filename) = field.file_name().map(str::to_string) {
                let bytes = field.bytes().await.map_err(|_| AppError::BadRequest("Envoi du fichier interrompu."))?;
                if !filename.is_empty() && !bytes.is_empty() {
                    files.insert(name, Upload { filename, bytes: bytes.to_vec() });
                }
            } else {
                let value = field.text().await.map_err(|_| AppError::BadRequest("Champ illisible."))?;
                fields.insert(name, value);
            }
        }
        let form = Self { fields, files };
        session::verify_csrf(session, form.get("_csrf_token")).await?;
        Ok(form)
    }

    pub fn get(&self, name: &str) -> &str {
        self.fields.get(name).map(String::as_str).unwrap_or("")
    }

    /// Texte facultatif : vide → None.
    pub fn text(&self, name: &str) -> Option<String> {
        let v = self.get(name).trim();
        (!v.is_empty()).then(|| v.to_string())
    }

    pub fn required(&self, name: &str, label: &str, errors: &mut Vec<String>) -> Option<String> {
        let v = self.text(name);
        if v.is_none() {
            errors.push(format!("Le champ « {label} » est obligatoire."));
        }
        v
    }

    pub fn int(&self, name: &str, label: &str, errors: &mut Vec<String>) -> Option<i32> {
        let v = self.text(name)?;
        match v.parse::<i32>() {
            Ok(n) => Some(n),
            Err(_) => {
                errors.push(format!("Le champ « {label} » doit être un nombre."));
                None
            }
        }
    }

    pub fn date(&self, name: &str, label: &str, errors: &mut Vec<String>) -> Option<NaiveDate> {
        let v = self.text(name)?;
        match NaiveDate::parse_from_str(&v, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => {
                errors.push(format!("Le champ « {label} » n'est pas une date valide."));
                None
            }
        }
    }

    /// Case « OUI / NON » ou « Traité / Non traité » : "1" → true.
    pub fn flag(&self, name: &str) -> bool {
        self.get(name) == "1"
    }

    pub fn file(&mut self, name: &str) -> Option<Upload> {
        self.files.remove(name)
    }
}

/// Filtre de période des listes : `MM/JJ/AAAA - MM/JJ/AAAA` (format par défaut du daterangepicker,
/// lu par `strtotime` dans l'original).
pub fn parse_range(dates: Option<&str>) -> Option<(NaiveDate, NaiveDate)> {
    let s = dates?.trim();
    let (a, b) = s.split_once(" - ")?;
    let parse = |x: &str| NaiveDate::parse_from_str(x.trim(), "%m/%d/%Y").ok();
    Some((parse(a)?, parse(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range() {
        let (a, b) = parse_range(Some("11/01/2026 - 11/25/2026")).unwrap();
        assert_eq!(a, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        assert_eq!(b, NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
        assert!(parse_range(Some("n'importe quoi")).is_none());
        assert!(parse_range(None).is_none());
    }
}
