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

        // Courrier arrivé
        "app_carrive_index" => "/admin/courriers/arrives",
        "app_carrive_dircab" => "/admin/courriers/arrives/dircab",
        "app_carrivearchive" => "/admin/courriers/arrives/archives",
        "app_carrive_archive" => "/admin/courriers/arrives/classes",
        "app_carrive_new" => "/admin/courriers/arrives/nouveau",
        "app_courrier_confi" => "/admin/courriers/arrives/nouveau/confidentiel",
        "app_carrive_show" => "/admin/courriers/arrives/{id}",
        "app_carrive_edit" => "/admin/courriers/arrives/{id}/modifier",
        "app_carrive_editcab" => "/admin/courriers/arrives/{id}/modifier/cab",
        "courrier_delete" => "/admin/courriers/arrives/{id}/supprimer",
        "courrier_deletedircab" => "/admin/courriers/arrives/{id}/supprimer/cab",
        "app_caarchive" => "/admin/courriers/arrives/{id}/archiver",
        "app_nonapp_caarchive" => "/admin/courriers/arrives/{id}/desarchiver",
        "app_export_fiche" => "/admin/courriers/arrives/{id}/fiche",

        // Imputations
        "app_imputation_index" => "/admin/imputations",
        "app_imputation_archive" => "/admin/imputations/classees",
        "app_archive" => "/admin/imputations/archives",
        "app_imputation_archivep" => "/admin/imputations/archives/perso",
        "app_imputation_dest" => "/admin/imputations/nouvelle/{id}",
        "app_imputation_sc" => "/admin/imputations/nouvelle/{id}/sc",
        "app_imputation_reimp" => "/admin/imputations/transmettre/{id}",
        "app_imputation_show" => "/admin/imputations/{id}",
        "app_imputation_edit" => "/admin/imputations/{id}/modifier",
        "app_imputation_retour" => "/admin/imputations/{id}/retour",
        "app_imputation_obs" => "/admin/imputations/{id}/observation",
        "app_final_doc" => "/admin/imputations/{id}/document-final",
        "app_imput_delete" => "/admin/imputations/{id}/supprimer",
        "app_traite" => "/admin/imputations/{id}/traite",
        "app_nontraite" => "/admin/imputations/{id}/non-traite",
        "app_imparchive" => "/admin/imputations/{id}/archiver",
        "app_nonapp_imparchive" => "/admin/imputations/{id}/desarchiver",

        // Espace service
        "app_destinataire_show" => "/admin/services/{id}",
        _ => return None,
    })
}
