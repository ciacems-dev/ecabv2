# ECAB-MS — Inventaire de l'application Symfony et plan de migration Rust

_Analyse du 2026-11-25, à partir du code déployé (`source/`) et du dump `ecab-db.sql.gz`._

## 1. Vue d'ensemble

| | |
|---|---|
| Métier | Gestion du courrier d'un cabinet ministériel : courrier arrivé, imputations aux services, courrier départ, comptes rendus et diligences, missions, audiences |
| Stack actuelle | Symfony 6.3, PHP ≥ 8.1, Doctrine ORM 2.15, Twig, MariaDB 10.11, OpenLiteSpeed |
| Front | AdminLTE 3 (Bootstrap 4, jQuery, DataTables côté client, select2, daterangepicker, fullcalendar) |
| Taille | ~3 500 lignes d'entités, ~3 200 lignes de contrôleurs, 26 formulaires, 122 templates Twig |
| Données | 17 tables, ~20 500 lignes ; uploads : ~23 Go dans `public/uploads/` |
| Utilisateurs | 52 (8 `ROLE_SUPER_ADMIN`, 37 `ROLE_ADMIN`, 4 `ROLE_USER`, 3 autres) |
| Mots de passe | bcrypt `$2y$13` pour les 52 → vérifiables en Rust, **aucune réinitialisation nécessaire** |

## 2. Modèle de données (source de vérité : le dump)

```
zone 1─n pays 1─n user n─1 destinataire (= service / entité)
                     │
                     └─1─n imputation n─1 carrive (courrier arrivé) 1─n document
                                     n─1 destinataire
destinataire 1─n csortir (courrier départ : auteur, initiateur) n─1 typedepart
destinataire 1─n mission | 1─n audience 1─n rendvous
compterendu 1─n diligence
user 1─n loginattemp (historique de connexions)
```

| Table | Lignes | Rôle | Champs clés |
|---|---:|---|---|
| `carrive` | 4 336 | Courrier arrivé | `numero`, `datearrive`, `datecour`, `numcour`, `expeditaire`, `objet`, `courrier` (fichier), `archive`, `confi` (confidentiel), `action` (imputé), `deposant`, `destinataire_id` |
| `imputation` | 6 509 | Attribution d'un courrier à un service | `courrier_id`, `destinataire_id`, `responsable_id` (user), `date`, `delais`, `reponse`, `statut` (traité), `commentaire`, `etat`, `doctoure` + `dateretour` (retour du service), `observationdir` + `dateobs` (avis du directeur), `docfinal` + `datefinale`, `effet`, `archive`, `transmet` |
| `csortir` | 5 084 | Courrier départ | `numsortir`, `datesortir`, `objet`, `destinataire` (texte), `courrier` + `decharge` (fichiers), `auteur_id`, `initiateur_id`, `type_id`, `agent`, `statut`, `archive` |
| `user` | 52 | Comptes | `email` (unique), `roles` (JSON), `password`, `is_verified`, `nom`, `prenom`, `civilite`, `telephone`, `active`, `pays_id`, `destinataire_id` |
| `destinataire` | 36 | Services / entités | `libelle`, `statut` |
| `typedepart` | 17 | Types de courrier départ | `libelle`, `statut` |
| `pays` / `zone` | 272 / 8 | Référentiel | — |
| `compterendu` / `diligence` | 10 / 4 | Réunions et actions à suivre | fichiers `cr`, `listep` ; `taux`, `delai`, `etat` |
| `mission` | 1 | Ordres de mission | fichiers `ordrem`, `rapportm` |
| `audience` / `rendvous` | 3 / 0 | Demandes d'audience | — |
| `document` | 0 | Pièces jointes de courrier (inutilisé) | — |
| `loginattemp` | 4 159 | Historique connexions | `ip`, `date_start`, `date_end` (jamais rempli) |
| `messenger_messages`, `doctrine_migration_versions` | — | Technique | **à ne pas migrer** |

Remarques schéma :
- Aucun trigger, vue ni procédure stockée.
- `carrive.numero` n'est unique que côté application (`UniqueEntity`), pas en base. À vérifier avant d'ajouter une contrainte `UNIQUE` (doublons possibles).
- Booléens stockés en `tinyint(1)` nullable → `boolean` PostgreSQL ; décider `NOT NULL DEFAULT false` après contrôle des NULL.
- `user.roles` est un JSON dans une colonne texte → `jsonb` ou, mieux, une colonne `role` simple (un seul rôle par utilisateur en pratique).

## 3. Flux métier

**Courrier arrivé → imputation → retour → décision → classement**

1. Le **service courrier** enregistre un courrier arrivé (`/admin/courriers/arrives/new`) avec le scan PDF. Une variante « confidentielle » existe (`/new/confidentiel`) pour le cabinet.
2. Le **directeur de cabinet** (vue « Dircab ») impute le courrier à un service avec un délai (`/admin/imputation/new/{id}`) → `carrive.action = 1`.
3. Ré-imputation possible à un responsable précis (`/new/{id}/{responsable}`).
4. Le service répond : document de retour + date (`/{id}/retour`).
5. Le directeur ajoute une observation (`/{id}/edit/observation`), puis le document final (`/{id}/edit/docfinal`).
6. Marquage traité / non traité, archivage / désarchivage.
7. **Fiche de suivi PDF** d'un courrier (`/{id}/fiche/suivi`, dompdf).

**Courrier départ** : enregistrement avec fichier et décharge, statut, archivage.
**Comptes rendus** → **diligences** (actions avec délai, taux d'avancement).
**Missions** (ordre de mission, rapport), **audiences** (accepter / refuser / confirmer) et **rendez-vous**.
**Administration** : utilisateurs (création, mot de passe, activation, historique de connexions), services (`destinataire`), types de départ.

Tableau de bord `/admin` : compteurs + courriers non imputés, départs non traités, CR, diligences, audiences.

## 4. Routes

~120 routes, toutes sous `/admin` sauf `/login`, `/logout` et `/`. Modules (préfixe → contrôleur) :

| Préfixe | Actions |
|---|---|
| `/admin` | tableau de bord, page « devis » |
| `/admin/courriers/arrives` | liste (200 derniers), archives, vue Dircab, nouveau, nouveau confidentiel, voir, modifier, supprimer, archiver, fiche PDF |
| `/admin/imputation` | listes (en cours, classées, archives, archives perso), nouvelle (3 variantes), retour, observation, doc final, traiter, archiver, supprimer |
| `/admin/csortir` | liste, archives, nouveau, modifier, décharge, archiver, supprimer |
| `/admin/compte_rendu`, `/admin/diligence` | CRUD + archives |
| `/admin/mission` | CRUD + variantes service courrier / dircab / destinataire |
| `/admin/audience`, `/admin/rendvous` | CRUD + accepter / refuser / confirmer |
| `/admin/Admin/user` | utilisateurs, membres, mot de passe, activer / désactiver, historique connexions |
| `/admin/destinataire`, `/admin/typedepart` | référentiels CRUD + activer / désactiver |

Filtre commun : paramètre `?dates=JJ/MM/AAAA - JJ/MM/AAAA` (daterangepicker).

**Code mort (non routé ou cassé), à ne pas porter** : `DocumentController`, `SmsController` + `Service/SmsGenerator` (Twilio mal câblé, n'a jamais pu fonctionner), `Auth/LoginController`, `Auth/RegistrationController` + vérification d'email, `Mails/*` (SwiftMailer, non installé), `ImputationController::sendEmail` (route de test), `User1.php`, `Entity/Carrive copy 1.php`.

## 5. Sécurité et droits — l'existant

- Authentification : formulaire email + mot de passe avec CSRF, connexion journalisée dans `loginattemp`.
- Contrôle serveur : **uniquement** `^/admin` → `ROLE_ADMIN` (hiérarchie `SUPER_ADMIN > ADMIN > USER`).
- Les droits fins sont **seulement dans les templates**, selon le libellé du service de l'utilisateur : `"SERVICE COURRIER"`, `"DIRECTEUR DE CABINET"`, `"CHARGE D'…"`, `"Accueil"`, plus `is_granted("ROLE_SUPER_ADMIN")`. Ils masquent des menus mais **n'empêchent pas l'accès direct aux URL**.

### Problèmes relevés (à corriger dans la version Rust, certains urgents sur l'existant)

| # | Gravité | Problème |
|---|---|---|
| 1 | **Critique** | **Secrets en clair dans le code** : clés Twilio (`Service/SmsGenerator.php`), mot de passe SMTP d'un autre domaine (`Mails/Mailer.php`), mot de passe d'application Gmail (`.env`). → à révoquer et changer. |
| 2 | **Critique** | **Fichiers uploadés publics et devinables** : `public/uploads/…` est servi sans authentification ; les courriers arrivés sont nommés `année + numéro` (ex. `20252553.pdf`), y compris les confidentiels. |
| 3 | **Critique (probable, non testé)** | Seuls 2 formulaires sur 13 limitent le type de fichier (PDF). Les autres acceptent n'importe quoi dans un dossier web public servi par LiteSpeed avec PHP → risque d'exécution de code via un `.php` téléversé par un compte admin. |
| 4 | Élevée | Droits fins seulement visuels (voir ci-dessus) : tout `ROLE_ADMIN` (37 comptes) peut lire / modifier / supprimer n'importe quel courrier, y compris confidentiel, en tapant l'URL. |
| 5 | Élevée | `UserChecker` désactivé : un compte marqué **inactif peut toujours se connecter**. |
| 6 | Élevée | Production en `APP_ENV=dev` et `display_errors On` : traces d'erreurs détaillées visibles. |
| 7 | Moyenne | Suppressions, archivages et activations en **GET sans jeton CSRF** : un simple lien piégé peut déclencher l'action. |
| 8 | Moyenne | Uploads jusqu'à 1 Go, `memory_limit -1` : risque de saturation. |
| 9 | Faible | `LogoutListener` mal référencé → la date de déconnexion n'est jamais enregistrée. |
| 10 | Performance | Les listes chargent toutes les lignes (`findAll`, ex. 6 500 imputations) puis filtrent en Twig / DataTables. |

## 6. Architecture Rust proposée

```
ecab-rust/
├─ Cargo.toml
├─ migrations/            # sqlx migrate (SQL PostgreSQL)
├─ src/
│  ├─ main.rs             # config, pool, router, tracing
│  ├─ config.rs           # variables d'environnement
│  ├─ error.rs            # AppError → réponses HTTP
│  ├─ auth/               # login, sessions, bcrypt/argon2, CSRF, extracteur CurrentUser
│  ├─ authz.rs            # politique d'accès centrale (rôles + profil de service)
│  ├─ db/                 # requêtes SQLx par module
│  ├─ routes/             # un fichier par module (courrier_arrive, imputation, …)
│  ├─ files.rs            # stockage des uploads + téléchargement contrôlé
│  ├─ pdf.rs              # fiche de suivi
│  └─ templates.rs        # MiniJinja (+ filtres date, asset, csrf)
├─ templates/             # portés depuis Twig (syntaxe quasi identique)
├─ static/                # AdminLTE réutilisé tel quel
└─ tools/migrate-data/    # import MariaDB → PostgreSQL + vérifications
```

| Besoin | Choix |
|---|---|
| HTTP | `axum` + `tower-http` (static, compression, limites de taille) |
| Base | PostgreSQL 16 + `sqlx` (requêtes vérifiées à la compilation, `QueryBuilder` pour les filtres) |
| Vues | `minijinja` — templates Twig portés presque tels quels, même rendu AdminLTE |
| Sessions | `tower-sessions` avec stockage PostgreSQL |
| Mots de passe | `bcrypt` (vérifie les hash `$2y$` existants) ; rehash en argon2 à la connexion |
| CSRF | jeton par session, vérifié sur tout POST ; toutes les actions qui modifient passent en POST |
| Uploads | hors du dossier public, noms aléatoires (UUID), types autorisés en liste blanche, taille max raisonnable, servis via une route qui vérifie les droits. Mêmes noms de fichiers conservés pour l'existant |
| PDF | rendu HTML → PDF via Chromium headless ou `typst`, à trancher sur la fiche de suivi |
| Emails | `lettre` (SMTP) si les notifications sont réactivées |
| Déploiement | Dockerfile multi-stage → image légère, PostgreSQL et volume `uploads` sur Dokploy |

### Droits dans la version Rust
Remplacer les comparaisons de libellés par un **profil explicite** par service (`courrier`, `dircab`, `charge`, `accueil`, `service`), vérifié **côté serveur** sur chaque route :
- courrier confidentiel visible seulement par le cabinet ;
- un service ne voit que ses imputations ;
- seul le super admin gère les utilisateurs.

À valider avec vous : la matrice exacte « qui voit / fait quoi ».

## 7. Migration des données

1. Charger le dump dans un MariaDB local (Docker), puis importer avec **pgloader** dans un schéma de transit, ou via un outil Rust dédié.
2. Transformer vers le schéma cible : booléens, `roles` → `role`, dates `UTC`, séquences recalées sur les `AUTO_INCREMENT`.
3. Vérifier : nombre de lignes par table, clés étrangères, accents (table en `utf8mb3`), doublons de `carrive.numero`.
4. Uploads : `rsync` direct VPS → serveur Dokploy, mêmes chemins relatifs ; contrôle des fichiers référencés en base mais absents (et inversement).
5. Répétition complète avant la bascule, puis bascule avec gel des écritures.

## 8. Ordre de construction

1. Socle : config, pool, migrations, layout AdminLTE, login / logout, sessions, CSRF, historique de connexion.
2. Référentiels : services, types de départ, utilisateurs.
3. **Courrier arrivé + imputations** (cœur métier) + fichiers + fiche PDF.
4. Courrier départ.
5. Comptes rendus / diligences, missions, audiences / rendez-vous.
6. Tableau de bord.
7. Outil de migration des données + tests de parité, Dockerfile, déploiement Dokploy.

## 9. Questions ouvertes

1. La matrice des droits par service (qui voit le confidentiel, qui impute, qui archive…).
2. Les 4 `ROLE_USER` et les 3 comptes au rôle non standard : quel accès ? (aujourd'hui les `ROLE_USER` ne peuvent pas entrer dans `/admin`).
3. SMS et emails : à réactiver dans la nouvelle version ou à abandonner ?
4. Avez-vous les sources front non compilées ou un dépôt git ? (sinon on garde AdminLTE tel quel, ce qui suffit).
5. La page « devis » et `DEVIS_HEBERGEMENT_ECAB-MS1.pdf` sont-elles à garder ?
