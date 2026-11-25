# Courrier arrivé et imputations : règles de gestion

Règles relevées dans le code Symfony d'origine (contrôleurs, templates Twig, entités) et
reproduites dans la version Rust. Les écarts volontaires sont listés à la fin.

## Profils

Un utilisateur a un **rôle** (`ROLE_SUPER_ADMIN`, `ROLE_ADMIN`, `ROLE_USER`) et un **service**
(`destinataire`). Les droits fins dépendent du libellé du service, comme dans l'original.

| Profil | Définition | Comptes actifs (nov. 2026) |
|---|---|---|
| Super admin | `ROLE_SUPER_ADMIN` | directeur de cabinet (3), chef de cabinet, DSI |
| Directeur de cabinet | service « DIRECTEUR DE CABINET » | tous super admin |
| Service courrier | service « SERVICE COURRIER » | 2 (ROLE_ADMIN) |
| Service | tout autre service, `ROLE_ADMIN` | ~15 |
| `ROLE_USER` | aucun accès à `/admin` (règle d'origine) | 2 |

## Matrice d'accès (désormais vérifiée côté serveur)

| Action | Super admin | Service courrier | Dir. de cabinet | Autre service |
|---|:-:|:-:|:-:|:-:|
| Liste « Arrivées », saisie, modification, archivage, suppression, fiche PDF | ✔ | ✔ | ✔ (super admin) | ✘ |
| Vue « Dircab » (inclut les confidentiels), saisie depuis cette vue | ✔ | ✘ | ✔ | ✘ |
| Voir / télécharger un courrier **confidentiel** | ✔ | ✘ | ✔ | ✘ |
| Imputer un courrier (depuis « Arrivées ») | ✔ | ✔ | ✔ | ✘ |
| Listes des imputations, modifier, supprimer, traité / non traité | ✔ | ✔ | ✔ | ✘ |
| Observation de la hiérarchie, document final | ✔ | ✘ | ✔ | ✘ |
| Voir une imputation | ✔ | ✔ | ✔ | si imputée à son service ou transmise par lui |
| Répondre (retour + fichier) | ✔ | ✔ | ✔ | si imputée à son service |
| Archiver / désarchiver une imputation | ✔ | ✔ | ✔ | si imputée à son service |
| « Transmet à » (ré-imputer à un autre service) | ✔ | ✔ | ✔ | si le courrier est imputé à son service |
| Espace service (« Mes courriers imputés ») | tous les services | le sien | le sien | le sien |
| Fichier joint d'un courrier arrivé | ✔ | ✔ (hors confidentiel) | ✔ | si le courrier est imputé à son service |
| Fichiers de retour / document final | ✔ | ✔ | ✔ | si imputée à son service |

## Courrier arrivé

- **Numéro** : l'utilisateur saisit le numéro d'ordre (ex. `2479`), l'application le préfixe par
  l'année (`20262479`). Affichage : sans l'année (`2479`). Le numéro complet doit être unique.
- À la création : `create_at` et `datearrive` = maintenant (UTC), `action` = 0 (non imputé).
- Champs : service destinataire*, confidentiel* (OUI / NON), date d'arrivée*, numéro*, nom et
  téléphone du déposant, expéditeur*, objet, fichier PDF.
- Listes :

| Page | Contenu | Tri |
|---|---|---|
| Arrivées (service courrier) | non archivés, non confidentiels, année en cours, parmi les 200 derniers enregistrés | numéro décroissant |
| Arrivées (dircab) | non archivés, année en cours (confidentiels compris, badge rouge), parmi les 200 derniers | id décroissant |
| Archivages › Arrivées | non confidentiels de l'**année précédente** | date décroissante |
| Courriers classés › Arrivées | archivés | id décroissant |

- Recherche par période (`MM/JJ/AAAA - MM/JJ/AAAA`) sur la date d'arrivée : remplace la limite
  des 200, les autres filtres de la page s'appliquent toujours.
- Le bouton « Fiche de suivi » n'apparaît que si le courrier a au moins une imputation.

## Imputation

- Création : service destinataire, instruction (liste fixe), niveau d'importance (Extrêmement
  urgent / Urgent), délai, date d'imputation, notes. `transmet` = 0. Le courrier passe à
  `action` = 1.
- « Transmet à » : nouvelle imputation du même courrier, avec `responsable` = l'utilisateur qui
  transmet (n'apparaît alors plus dans la liste générale, qui ne montre que `responsable` vide).
- Retour du service : texte « état » + fichier ; `dateretour` = maintenant.
- Observation (super admin) : instruction, notes, état, observation ; `dateobs` = maintenant.
- Document final (super admin) : fichier + état + observation ; `datefinale` = maintenant si fichier.
- Listes :

| Page | Contenu | Tri |
|---|---|---|
| Imputations | `responsable` vide, imputées dans l'année en cours | id décroissant |
| Archivages › Imputations | `responsable` vide, toutes années | id décroissant |
| Courriers classés › Imputations | archivées | date décroissante |
| Espace service | imputations du service non archivées | id décroissant |
| Archives perso (`/archive/per`) | imputations du service archivées | id décroissant |

- Délai : cellule bleue si le délai n'est pas dépassé (un délai vide compte comme « aujourd'hui »),
  rose sinon.
- Notification (cloche) : imputations de son service, non traitées, de l'année, délai non dépassé.

## Écarts volontaires par rapport à l'original

| Original | Version Rust | Pourquoi |
|---|---|---|
| Droits seulement dans les menus | Vérifiés sur chaque route | n'importe quel admin pouvait tout lire et supprimer par l'URL |
| Fichiers publics `/uploads/…`, noms devinables | Route protégée `/fichiers/…`, nouveaux fichiers nommés aléatoirement (le nom affiché au téléchargement reste `année+numéro.pdf`) | confidentialité |
| Types de fichiers libres (retours, docs finaux) | Liste blanche : PDF, Word, Excel, OpenDocument, images | risque d'exécution de code |
| Suppressions, archivages, « traité » par simple lien (GET) | POST + jeton CSRF, confirmation | un lien piégé pouvait déclencher l'action |
| Bouton supprimer d'une liste : supprimait **le premier courrier** de la liste (fenêtre de confirmation partagée) | Supprime le courrier cliqué | bug grave |
| Suppression d'une imputation sans confirmation | Avec confirmation | erreur de clic |
| Suppression d'un courrier imputé : erreur serveur | Message « supprimez d'abord ses imputations » | lisibilité |
| Archivages › Arrivées : ne regardait que les **1 000 premiers** courriers de la base | Tous les courriers de l'année précédente | l'année précédente était presque entièrement invisible |
| Modifier le numéro d'un ancien courrier lui donnait l'année en cours (ou plantait) | L'année d'origine est conservée | bug |
| Notes / état / observation affichés en HTML brut | HTML nettoyé (balises sûres seulement) | injection de script |
| « Transmet à » : l'utilisateur transmetteur était pris dans l'URL | Toujours l'utilisateur connecté | usurpation |
| Espace d'un autre service accessible par l'URL | Réservé au service et aux super admins | confidentialité |
| Listes : chargement de toutes les imputations puis filtrage en Twig | Filtrage en SQL | performance |

# Courrier départ

Règles relevées dans `CsortirController`, `CsortirType`, `EditersortirType`, `DechargeType` et les
templates `admin/csortir/*`.

- Accès : service courrier ou super admin (menu « Gestion de Courriers › Départs »). Un service voit
  dans son espace les départs dont il est l'initiateur, et peut ouvrir leurs fichiers.
- Saisie : type*, date d'enregistrement*, numéro d'enregistrement* (saisi tel quel, sans unicité),
  objet, destinataire* (texte libre), signataire* (service), initiateur* (service), agent de dépôt,
  courrier (fichier), décharge (fichier). `datesortir` = maintenant à la création.
- Modification : mêmes champs ; objet et agent deviennent obligatoires (EditersortirType).
- Décharge : agent + fichier. Envoyer une décharge (saisie, modification ou page dédiée) passe le
  courrier à « envoyé » (`statut` = 1).
- Listes :

| Page | Contenu | Tri |
|---|---|---|
| Départs | non archivés de l'année en cours (année de la date d'enregistrement) | date décroissante |
| Archivages › Départs | non archivés de l'**année précédente** (l'original n'affiche nulle part les départs archivés) | date décroissante |

- Archiver n'est proposé qu'une fois la décharge jointe ; « désarchiver » est proposé depuis Archivages.

Écarts volontaires : suppression en POST avec confirmation (elle se faisait par un simple lien,
sans confirmation) ; fichiers en liste blanche (aucun contrôle à l'origine) et servis par la route protégée.
