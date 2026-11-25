# ECAB-MS (Rust)

Réécriture en Rust (Axum + PostgreSQL) de l'application Symfony ECAB-MS de gestion du courrier.
Les règles de gestion sont reprises à l'identique de l'application d'origine ; seules la sécurité
et les performances changent. Voir [docs/inventaire.md](docs/inventaire.md) et, pour le courrier arrivé et les imputations,
[docs/regles-courrier-arrive.md](docs/regles-courrier-arrive.md) (règles, matrice des droits, écarts volontaires).

## Démarrer en local

```bash
cp .env.example .env
docker compose -f docker-compose.dev.yml up -d   # PostgreSQL + MariaDB (chargée depuis ecab-db.sql.gz)
cargo run --bin import -- --reset                 # copie MariaDB → PostgreSQL et vérifie les comptages
cargo run                                         # http://127.0.0.1:8080
```

Prérequis : Rust, Docker, et Chrome ou Chromium pour la fiche de suivi PDF (`CHROME_BIN`).
Les fichiers joints sont lus dans `UPLOADS_DIR` (mêmes sous-dossiers que `public/uploads` d'origine).

Créer un compte de test (le mot de passe ne passe pas en argument) :

```bash
ECAB_NEW_PASSWORD='un-mot-de-passe-long' cargo run -- create-user test@example.test ROLE_SUPER_ADMIN "SERVICE COURRIER"
```

## Organisation

| Chemin | Contenu |
|---|---|
| `src/auth.rs` | connexion, déconnexion, contrôle d'accès `/admin`, droits par profil |
| `src/handlers/courrier_arrive.rs` | courrier arrivé : listes, saisie, archivage, fiche de suivi PDF |
| `src/handlers/imputation.rs` | imputations : création, transmission, retour, observation, document final |
| `src/handlers/destinataire.rs` | espace d'un service (courriers imputés et initiés) |
| `src/files.rs` | fichiers joints : stockage hors web, téléchargement contrôlé |
| `src/pdf.rs` | HTML → PDF avec Chromium sans interface |
| `src/layout.rs` | contexte commun du layout (utilisateur, CSRF, flash, notifications) |
| `src/templates.rs` | MiniJinja + fonctions Twig (`asset`, `path`, `is_granted`, `date`, `slice`) |
| `src/routes.rs` | noms de routes Symfony → chemins |
| `src/bin/import.rs` | import des données MariaDB → PostgreSQL |
| `migrations/` | schéma PostgreSQL (sqlx) |
| `templates/` | templates portés depuis Twig |
| `static/assets/` | AdminLTE et plugins, repris tels quels |

`source/` et `ecab-db.sql.gz` (code et données d'origine) ne sont jamais commités : ils contiennent des secrets et des données personnelles.
