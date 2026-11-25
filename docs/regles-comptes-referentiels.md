# Comptes et référentiels : règles de gestion

Règles relevées dans `AdminUserController`, `DestinataireController`, `TypedepartController`, les
formulaires `UserType`, `UserEditType`, `UserpsType`, `DestinataireType`, `TypedepartType` et les
templates `admin/user`, `admin/destinataire`, `admin/typedepart`.

## Accès

| Page | Qui (menu d'origine, désormais vérifié côté serveur) |
|---|---|
| Comptes › Administrateurs, Utilisateurs, fiche, création, modification, activation | super admin |
| Comptes › DESTINATAIRES (services), Cabinet › Suivi des imputations | super admin |
| Comptes › Historique des connexions | super admin |
| Types de départ (bouton de la page Départs) | service courrier, super admin |
| Changer son mot de passe (menu du profil) | chacun pour son compte ; un super admin pour tout compte |

## Utilisateurs

- **Administrateurs** : rôle `ROLE_ADMIN`, `ROLE_TECH` ou `ROLE_SUPER_ADMIN` ; **Utilisateurs** :
  `ROLE_USER`. Colonnes : civilité nom prénoms, pays (drapeau), email, téléphone, service.
- Création : service, nom, prénoms, civilité (M. / Mme / Mlle), pays, téléphone, email (unique :
  « Il existe déjà un compte avec ce email »), mot de passe + confirmation. Le compte est créé avec
  `ROLE_ADMIN` et vérifié.
- Modification : mêmes champs sans mot de passe, plus les rôles « Editeur » (`ROLE_USER`) et
  « Administrateur » (`ROLE_ADMIN`).
- Activer / désactiver un compte.
- Fiche : identité, imputations de son service, tous les comptes rendus (avec leurs diligences),
  toutes les diligences.
- Historique : toutes les connexions (début, fin, nom, service), les plus récentes d'abord.

## Services (destinataires)

- Libellé et profil. Activer / désactiver.
- **Suivi des imputations** : pour chaque service, nombre d'imputations directes (sans
  `responsable`), nombre traitées avec le taux (arrondi à l'unité), nombre non traitées.
- **Profil** du service (nouveau, voir ci-dessous) : service ordinaire, service courrier,
  directeur de cabinet, chargé d'études ou accueil.

### Profil des services

Dans l'application d'origine, les droits dépendaient du **libellé exact** de quatre services :
27 comparaisons du type `app.user.destinataire.libelle == "SERVICE COURRIER"` dans les templates
(17 dans le menu, 8 dans le tableau de bord, 2 dans les fiches). Renommer un service retirait ses
droits à tous ses utilisateurs, et une faute de frappe dans le libellé suffisait à les perdre.

La version Rust ajoute une colonne `destinataire.profil` qui porte ces droits :

| Profil | Droits | Service d'origine |
|---|---|---|
| `courrier` | gestion du courrier arrivé et départ, imputations | SERVICE COURRIER |
| `dircab` | imputations, observation (avec le rôle super admin : vue Dircab) | DIRECTEUR DE CABINET |
| `etudes` | comptes rendus, diligences | CHARGE D'ETUDES |
| `accueil` | menu réduit (pas d'espace service) | Accueil |
| *(vide)* | service ordinaire | tous les autres |

- La migration `20261126000001_profil_service.sql` et l'outil d'import déduisent le profil des
  libellés d'origine : rien ne change pour les utilisateurs à la bascule.
- Le code et les templates ne lisent plus jamais le libellé pour décider d'un droit.
- Un super admin choisit le profil dans le formulaire du service ; le libellé se modifie librement.
- Plusieurs services peuvent avoir le même profil (par exemple deux bureaux du courrier).

## Types de départ

- Libellé + statut (Activé / Désactivé). Création, modification, suppression.

## Écarts volontaires

| Original | Version Rust | Pourquoi |
|---|---|---|
| N'importe quel admin pouvait changer le mot de passe de **n'importe quel compte** (id dans l'URL) | Son propre mot de passe (avec le mot de passe actuel), ou tout compte pour un super admin | prise de contrôle de comptes |
| Modifier un super admin avec le formulaire le **rétrogradait** en admin (rôle absent des choix) | Le rôle super admin est conservé | bug |
| Mot de passe de 6 caractères minimum | 8 caractères minimum | sécurité |
| Compte créé sans statut actif (connexion possible seulement parce que le contrôle était désactivé) | Compte créé actif | le contrôle des comptes inactifs est maintenant appliqué |
| Désactiver un compte : message « Audience désactivé » | « Utilisateur désactivé » ; impossible de se désactiver soi-même | lisibilité, éviter de se bloquer |
| Suppression d'un type de départ utilisé : erreur serveur | Message explicite | lisibilité |
| Activation, désactivation, suppression par simple lien (GET) | POST + jeton CSRF | lien piégé |
| Page « voir » d'un type de départ (template cassé, jamais liée) | Non reprise | code mort |
| Droits liés au libellé exact de quatre services | Droits liés au profil du service, libellé libre | renommer un service retirait ses droits |
