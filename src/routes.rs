//! Registre des noms de routes (repris de l'application Symfony) → chemins.
//! Sert à la fonction `path()` des templates, pour que les templates portés gardent
//! leurs `path('app_xxx', {'id': ..})` tels quels.
//!
//! Une route pas encore portée renvoie `#a-venir` : le layout affiche ces liens grisés,
//! avec un message au clic, au lieu de casser la page.

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

        // Courrier départ
        "app_csortir_index" => "/admin/courriers/departs",
        "app_csortir_archive" => "/admin/courriers/departs/archives",
        "app_csortir_new" => "/admin/courriers/departs/nouveau",
        "app_csortir_show" => "/admin/courriers/departs/{id}",
        "app_csortir_edit" => "/admin/courriers/departs/{id}/modifier",
        "app_csortir_decharge" => "/admin/courriers/departs/{id}/decharge",
        "app_cs_delete" => "/admin/courriers/departs/{id}/supprimer",
        "app_csarchive" => "/admin/courriers/departs/{id}/archiver",
        "app_nonapp_csarchive" => "/admin/courriers/departs/{id}/desarchiver",

        // Espace service et services
        "app_destinataire_show" => "/admin/services/{id}",
        "app_destinataire" => "/admin/services",
        "app_destinataire_index" => "/admin/services/suivi",
        "app_destinataire_new" => "/admin/services/nouveau",
        "app_destinataire_edit" => "/admin/services/{id}/modifier",
        "app_active" => "/admin/services/{id}/activer",
        "app_desactive" => "/admin/services/{id}/desactiver",

        // Types de départ
        "app_typedepart_index" => "/admin/types-depart",
        "app_typedepart_new" => "/admin/types-depart/nouveau",
        "app_typedepart_edit" => "/admin/types-depart/{id}/modifier",
        "app_type_delete" => "/admin/types-depart/{id}/supprimer",

        // Comptes rendus et diligences
        "app_compterendu_index" => "/admin/comptes-rendus",
        "app_compterendu_archive" => "/admin/comptes-rendus/archives",
        "app_compterendu_new" => "/admin/comptes-rendus/nouveau",
        "app_compterendu_show" => "/admin/comptes-rendus/{id}",
        "app_compterendu_edit" => "/admin/comptes-rendus/{id}/modifier",
        "app_crdelete" => "/admin/comptes-rendus/{id}/supprimer",
        "app_crarchive" => "/admin/comptes-rendus/{id}/archiver",
        "app_nonapp_crarchive" => "/admin/comptes-rendus/{id}/desarchiver",
        "app_diligence_index" => "/admin/diligences",
        "app_diligence_archive" => "/admin/diligences/archives",
        "app_diligence_new" => "/admin/diligences/nouvelle",
        "app_diligence_cr" => "/admin/diligences/nouvelle/{id}",
        "app_diligence_show" => "/admin/diligences/{id}",
        "app_diligence_edit" => "/admin/diligences/{id}/modifier",
        "diligence_delete" => "/admin/diligences/{id}/supprimer",
        "app_dilarchive" => "/admin/diligences/{id}/archiver",
        "app_nonapp_dilarchive" => "/admin/diligences/{id}/desarchiver",

        // Audiences et rendez-vous
        "app_audience_index" => "/admin/audiences",
        "app_audience_new" => "/admin/audiences/nouvelle",
        "app_audience_show" => "/admin/audiences/{id}",
        "app_audience_edit" => "/admin/audiences/{id}/modifier",
        "app_confirme" => "/admin/audiences/{id}/accepter",
        "app_nonconfirme" => "/admin/audiences/{id}/refuser",
        "app_audience_delete" => "/admin/audiences/{id}/supprimer",
        "app_rendvous_index" => "/admin/rendez-vous",
        "app_rendvous_new" => "/admin/rendez-vous/nouveau",
        "app_rendvous_edit" => "/admin/rendez-vous/{id}/modifier",
        "app_rendvous_delete" => "/admin/rendez-vous/{id}/supprimer",

        // Comptes
        "app_user_admin" => "/admin/utilisateurs",
        "app_user_membre" => "/admin/utilisateurs/membres",
        "app_user_new" => "/admin/utilisateurs/nouveau",
        "app_user_show" => "/admin/utilisateurs/{id}",
        "app_user_edit" => "/admin/utilisateurs/{id}/modifier",
        "app_password_edit" => "/admin/utilisateurs/{id}/mot-de-passe",
        "app_active_user" => "/admin/utilisateurs/{id}/activer",
        "app_desactive_user" => "/admin/utilisateurs/{id}/desactiver",
        "app_connexion_histo" => "/admin/utilisateurs/historique",
        _ => return None,
    })
}
