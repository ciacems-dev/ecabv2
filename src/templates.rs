//! Moteur de templates MiniJinja, avec les fonctions et filtres Twig utilisés par
//! les templates d'origine : `asset()`, `path()`, `is_granted()`, `|date('d-m-Y')`, `|slice(0, 25)`.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use minijinja::value::Value;
use minijinja::{Environment, Error, ErrorKind, State, UndefinedBehavior};

pub fn build(templates_dir: &str) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader(templates_dir));
    // Proche du comportement de Twig en production : un attribut absent vaut « vide » au lieu de planter.
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    // Comme Twig : une valeur nulle s'affiche vide (MiniJinja afficherait « none »).
    env.set_formatter(|out, state, value| {
        if value.is_none() || value.is_undefined() {
            Ok(())
        } else {
            minijinja::escape_formatter(out, state, value)
        }
    });

    // URL produite par l'application : marquée sûre, sinon l'échappement HTML transforme `/` en `&#x2f;`
    // et casse les `url(...)` dans les balises <style>.
    env.add_function("asset", |path: String| {
        Value::from_safe_string(format!("/{}", encode_path(path.trim_start_matches('/'))))
    });
    env.add_function("path", path_fn);
    env.add_function("is_granted", is_granted);
    env.add_filter("date", date_filter);
    env.add_filter("slice", slice_filter);
    env.add_filter("sanitize", sanitize_filter);
    env.add_filter("number_format", number_format_filter);
    env.add_function("file_url", |dir: String, name: Option<String>| {
        Value::from_safe_string(name.filter(|n| !n.is_empty()).map(|n| crate::files::url(&dir, &n)).unwrap_or_default())
    });
    env
}

/// Cible des liens vers les modules pas encore portés (repérée par le layout).
pub const A_VENIR: &str = "#a-venir";

/// `path('nom_route', {'id': 3})` → `/admin/.../3`. Les paramètres non utilisés dans le motif
/// sont ajoutés en query string, comme Symfony.
fn path_fn(name: String, params: Option<Value>) -> Result<Value, Error> {
    path_string(name, params).map(Value::from_safe_string)
}

/// Encode un chemin de fichier en gardant les `/` (noms d'uploads avec espaces ou accents).
fn encode_path(p: &str) -> String {
    p.split('/').map(encode).collect::<Vec<_>>().join("/")
}

fn path_string(name: String, params: Option<Value>) -> Result<String, Error> {
    let Some(pattern) = crate::routes::pattern(&name) else {
        // Lien vers un module pas encore porté : le layout l'affiche grisé avec un message au clic.
        tracing::debug!(route = %name, "route pas encore portée");
        return Ok(A_VENIR.into());
    };
    let mut url = pattern.to_string();
    let mut query = Vec::new();
    if let Some(params) = params
        && let Ok(keys) = params.try_iter() {
            for key in keys {
                let k = key.to_string();
                let v = params.get_item(&key)?.to_string();
                let placeholder = format!("{{{k}}}");
                if url.contains(&placeholder) {
                    url = url.replace(&placeholder, &encode(&v));
                } else {
                    query.push(format!("{}={}", encode(&k), encode(&v)));
                }
            }
        }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    Ok(url)
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Hiérarchie des rôles reprise de `security.yaml` : SUPER_ADMIN ⊃ ADMIN ⊃ USER.
pub fn role_granted(user_roles: &[String], wanted: &str) -> bool {
    user_roles.iter().any(|r| {
        r == wanted
            || (r == "ROLE_SUPER_ADMIN" && matches!(wanted, "ROLE_ADMIN" | "ROLE_USER" | "ROLE_ALLOWED_TO_SWITCH"))
            || (r == "ROLE_ADMIN" && wanted == "ROLE_USER")
    })
}

fn is_granted(state: &State, role: String) -> bool {
    let Some(app) = state.lookup("app") else { return false };
    let Ok(roles) = app.get_attr("user").and_then(|u| u.get_attr("roles")) else { return false };
    let roles: Vec<String> = match roles.try_iter() {
        Ok(it) => it.map(|r| r.to_string()).collect(),
        Err(_) => return false,
    };
    role_granted(&roles, &role)
}

fn parse_date(v: &Value) -> Option<NaiveDateTime> {
    let s = v.as_str()?;
    if s == "now" {
        return Some(chrono::Utc::now().naive_utc());
    }
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))
        .or_else(|| {
            // Heure seule (colonnes `time`) : rattachée à une date arbitraire
            chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.f")
                .ok()
                .map(|t| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap().and_time(t))
        })
}

/// Filtre `date` au format PHP (`d-m-Y`, `Y`, `H:i`…), comme Twig.
/// Écart volontaire : une valeur vide donne une chaîne vide (Twig affichait la date du jour).
fn date_filter(value: Value, format: Option<String>) -> Result<String, Error> {
    if value.is_undefined() || value.is_none() {
        return Ok(String::new());
    }
    let dt = parse_date(&value)
        .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, format!("date invalide : {value}")))?;
    let format = format.unwrap_or_else(|| "F j, Y H:i".into());
    let mut out = String::new();
    for c in format.chars() {
        match c {
            'd' => out.push_str(&format!("{:02}", dt.day())),
            'j' => out.push_str(&dt.day().to_string()),
            'm' => out.push_str(&format!("{:02}", dt.month())),
            'n' => out.push_str(&dt.month().to_string()),
            'Y' => out.push_str(&dt.year().to_string()),
            'y' => out.push_str(&format!("{:02}", dt.year() % 100)),
            'H' => out.push_str(&format!("{:02}", dt.hour())),
            'G' => out.push_str(&dt.hour().to_string()),
            'i' => out.push_str(&format!("{:02}", dt.minute())),
            's' => out.push_str(&format!("{:02}", dt.second())),
            'F' => out.push_str(MOIS[dt.month0() as usize]),
            other => out.push(other),
        }
    }
    Ok(out)
}

const MOIS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];

/// `slice(debut, longueur)` sur une chaîne, comme Twig (compte en caractères, pas en octets).
fn slice_filter(value: Value, start: i64, length: Option<i64>) -> String {
    if value.is_undefined() || value.is_none() {
        return String::new();
    }
    let s = value.to_string();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let start = if start < 0 { (n + start).max(0) } else { start.min(n) } as usize;
    let end = match length {
        Some(l) if l >= 0 => (start + l as usize).min(chars.len()),
        Some(l) => ((n + l).max(start as i64)) as usize,
        None => chars.len(),
    };
    chars[start..end].iter().collect()
}

/// `number_format(decimales, separateur_decimal, separateur_milliers)` comme Twig / PHP.
fn number_format_filter(value: Value, decimals: Option<usize>, dec_point: Option<String>, thousands: Option<String>) -> String {
    if value.is_undefined() || value.is_none() {
        return "0".into();
    }
    let n: f64 = f64::try_from(value.clone()).ok().or_else(|| value.to_string().trim().parse().ok()).unwrap_or(0.0);
    let decimals = decimals.unwrap_or(0);
    let dec_point = dec_point.unwrap_or_else(|| ".".into());
    let thousands = thousands.unwrap_or_else(|| ",".into());
    let fixed = format!("{:.*}", decimals, n.abs());
    let (int_part, frac_part) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push_str(&thousands);
        }
        grouped.push(c);
    }
    let sign = if n < 0.0 && fixed.chars().any(|c| c != '0' && c != '.') { "-" } else { "" };
    if frac_part.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}{dec_point}{frac_part}") }
}

/// Remplace `|raw` de l'original : garde la mise en forme (paragraphes, gras, listes…)
/// mais retire scripts, attributs d'événements et liens `javascript:`.
fn sanitize_filter(value: Value) -> Value {
    if value.is_undefined() || value.is_none() {
        return Value::from("");
    }
    Value::from_safe_string(ammonia::clean(&value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formats() {
        assert_eq!(date_filter(Value::from("2025-09-16T08:22:10"), Some("d-m-Y".into())).unwrap(), "16-09-2025");
        assert_eq!(date_filter(Value::from("2025-09-16"), Some("Y".into())).unwrap(), "2025");
        assert_eq!(date_filter(Value::from(()), Some("d-m-Y".into())).unwrap(), "");
    }

    #[test]
    fn slice_chars() {
        assert_eq!(slice_filter(Value::from("MINISTÈRE DE L'ÉCONOMIE"), 0, Some(10)), "MINISTÈRE ");
        assert_eq!(slice_filter(Value::from(()), 0, Some(10)), "");
    }

    #[test]
    fn number_format() {
        assert_eq!(number_format_filter(Value::from(66.6), Some(0), Some(".".into()), Some(",".into())), "67");
        assert_eq!(number_format_filter(Value::from(1234567.891), Some(2), Some(",".into()), Some(" ".into())), "1 234 567,89");
        assert_eq!(number_format_filter(Value::from("50"), None, None, None), "50");
        assert_eq!(number_format_filter(Value::from(()), Some(0), None, None), "0");
    }

    #[test]
    fn sanitize() {
        let out = sanitize_filter(Value::from("<p>Bonjour</p><script>alert(1)</script><img src=x onerror=alert(1)>"));
        assert_eq!(out.to_string(), "<p>Bonjour</p><img src=\"x\">");
    }

    /// Tous les templates doivent compiler (erreur de syntaxe détectée avant la mise en ligne).
    #[test]
    fn all_templates_compile() {
        let env = build("templates");
        let mut stack = vec![std::path::PathBuf::from("templates")];
        let mut n = 0;
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "html") {
                    let name = path.strip_prefix("templates").unwrap().to_string_lossy().replace('\\', "/");
                    if let Err(e) = env.get_template(&name) {
                        panic!("{name} : {e:#}");
                    }
                    n += 1;
                }
            }
        }
        assert!(n > 20, "{n} templates seulement");
    }

    #[test]
    fn roles() {
        let sa = vec!["ROLE_SUPER_ADMIN".to_string()];
        let a = vec!["ROLE_ADMIN".to_string()];
        assert!(role_granted(&sa, "ROLE_ADMIN"));
        assert!(!role_granted(&a, "ROLE_SUPER_ADMIN"));
        assert!(role_granted(&a, "ROLE_USER"));
    }
}
