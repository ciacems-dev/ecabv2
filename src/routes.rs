//! Registre des noms de routes (repris de l'application Symfony) → chemins.
//! Sert à la fonction `path()` des templates, pour que les templates portés gardent
//! leurs `path('app_xxx', {'id': ..})` tels quels.
//!
//! Une route pas encore portée renvoie `#` (et un avertissement dans les logs) :
//! les liens restent inertes au lieu de casser la page.

pub fn pattern(name: &str) -> Option<&'static str> {
    Some(match name {
        "app_login" => "/login",
        "app_logout" => "/logout",
        "app_home" => "/",
        "app_admin" => "/admin",
        _ => return None,
    })
}
