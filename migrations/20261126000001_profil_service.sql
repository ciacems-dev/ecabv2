-- Profil d'un service, indépendant de son libellé.
--
-- Dans l'application Symfony, les droits dépendaient du libellé exact de certains services
-- (27 comparaisons « libelle == "SERVICE COURRIER" » dans les templates) : renommer un service
-- retirait ses droits. Le profil porte désormais ces droits ; le libellé redevient un simple nom.

ALTER TABLE destinataire
    ADD COLUMN profil varchar(20)
    CHECK (profil IN ('courrier', 'dircab', 'etudes', 'accueil'));

COMMENT ON COLUMN destinataire.profil IS
    'courrier = service courrier, dircab = directeur de cabinet, etudes = chargé d''études, accueil = accueil, NULL = service ordinaire';

-- Reprise des libellés de l'application d'origine (même requête dans l'outil d'import).
UPDATE destinataire SET profil = CASE libelle
    WHEN 'SERVICE COURRIER' THEN 'courrier'
    WHEN 'DIRECTEUR DE CABINET' THEN 'dircab'
    WHEN 'CHARGE D''ETUDES' THEN 'etudes'
    WHEN 'Accueil' THEN 'accueil'
END;
