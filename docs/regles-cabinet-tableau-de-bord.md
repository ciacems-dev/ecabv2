# Comptes rendus, diligences, audiences et tableau de bord : règles de gestion

Règles relevées dans `CompterenduController`, `DiligenceController`, `AudienceController`,
`RendvousController`, `AdminHomeController`, les formulaires associés et les templates
`admin/compterendu`, `admin/diligence`, `admin/audience`, `admin/rendvous`, `admin/home`.

## Accès (menus d'origine, désormais vérifiés côté serveur)

| Module | Qui |
|---|---|
| Comptes rendus, diligences (et leurs archives) | profil `etudes` (chargé d'études) ou super admin |
| Audiences, rendez-vous | super admin ; profil `accueil` (sa page d'accueil liste et saisit les audiences) |
| Accepter / refuser / supprimer une audience | super admin |

## Comptes rendus

- Champs : date du conseil*, heure de début*, heure de fin*, objet, lieu*, présidence*
  (MINISTRE / DIRCAB / IG), fiche de rapport (fichier), liste de présence (fichier), rédacteur*,
  statut* (Public / Privé). `create_at` = maintenant.
- Liste : non archivés, date décroissante, avec leurs diligences et un bouton pour en ajouter une.
- Archiver ; page détail avec les diligences.

## Diligences

- Champs : titre*, sujet, ressource(s)*, début d'exécution, délai d'exécution, taux, état*
  (Réaliser / Non Réaliser), observation. `create_at` = maintenant. Rattachée à un compte rendu
  si elle est créée depuis celui-ci.
- Liste : non archivées dont le début d'exécution est dans l'année en cours (début vide = année en
  cours), début décroissant. Archivage proposé une fois l'état « Réaliser ».
- Archives : diligences archivées, désarchivage si réalisée.

## Audiences et rendez-vous

- Audience : destinataire*, date*, heure, nom, téléphone, email, service, qualité, rendez-vous
  (OUI / NON), objet. Liste par id décroissant ; statut Accepté / Refusé.
- Rendez-vous : date début, date fin, heure début, heure fin, lieu (« Imputation budget »),
  message. Liste, saisie, modification (table vide en production).

## Tableau de bord

| Bloc | Qui le voit | Contenu |
|---|---|---|
| Compteurs | super admin (hors profil accueil) | courriers arrivés, départs, comptes rendus |
| Onglets « Dossiers imputés », « Comptes Rendus », « Diligences » | tous sauf profils accueil et courrier | imputations de son service non archivées de l'année ; comptes rendus publics de l'année ; toutes les diligences |
| Courriers départ non envoyés | profil courrier | départs de l'année sans décharge |
| Audiences | profil accueil | toutes les audiences, bouton « Nouveau » |

## Écarts volontaires

| Original | Version Rust | Pourquoi |
|---|---|---|
| « Archivages › Comptes Rendus » affichait les comptes rendus **non** archivés (copie de la liste) ; un compte rendu archivé n'apparaissait plus nulle part | Affiche les comptes rendus archivés, avec « désarchiver » | bug |
| Badges Accepté / Refusé et bouton supprimer des audiences sans effet (fenêtre absente) | Fonctionnels, en POST avec confirmation | fonctions annoncées mais inopérantes |
| Fiche audience et pages rendez-vous sur le gabarit Symfony vide (pages brutes, sans menu) ; création d'un rendez-vous depuis une audience bloquée par un `dd()` de débogage | Pages dans le gabarit de l'application | pages inutilisables |
| Date d'audience facultative dans le formulaire mais obligatoire en base (erreur serveur) | Obligatoire dans le formulaire | erreur serveur |
| Suppressions, archivages par simple lien (GET), sans confirmation | POST + jeton CSRF, confirmation | lien piégé, erreur de clic |
| Fichiers des comptes rendus sans contrôle de type, publics | Liste blanche, route protégée | sécurité |
| Tableau de bord : comparaisons de libellés | Profils | voir docs/regles-comptes-referentiels.md |
