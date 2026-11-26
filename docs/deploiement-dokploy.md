# Déploiement sur Dokploy

L'image est construite par Dokploy à partir du `Dockerfile` du dépôt (branche `main`).
Elle contient deux binaires : `/app/ecab` (le serveur) et `/app/import` (la migration des données).

## 0. Avant de commencer

- **Le dépôt GitHub `ciacems-dev/ecabv2` doit rester privé** tant que l'ancienne application tourne :
  `docs/` décrit ses failles.
- **Même VPS que l'ancienne application** : OpenLiteSpeed est placé derrière Traefik (réglé
  hors de ce dépôt). Les deux applications cohabitent : la nouvelle se teste sur un sous-domaine
  pendant que `ecab.mds-dsi.org` reste servi par l'ancienne, puis le domaine bascule.

## 1. Base PostgreSQL

Dans le projet Dokploy : **Create Service → Database → PostgreSQL** (version 16).

- Nom de la base : `ecab` ; utilisateur : `ecab` ; mot de passe : long et aléatoire.
- Noter l'**adresse interne** affichée par Dokploy (« Internal Host »), utilisée dans `DATABASE_URL`.
- Activer les sauvegardes automatiques de la base (onglet Backups).

## 2. Application

**Create Service → Application.**

1. **Source** : provider GitHub. Si le compte n'est pas encore relié : Dokploy → Settings → Git →
   GitHub → installer l'application GitHub de Dokploy sur le compte `ciacems-dev`, avec accès au dépôt
   `ecabv2`. Puis dépôt `ecabv2`, branche **`main`**.
2. **Build Type** : **Dockerfile** (chemin `Dockerfile`, contexte `.`).
3. **Environment** :

   ```env
   DATABASE_URL=postgres://ecab:MOT_DE_PASSE@ADRESSE_INTERNE:5432/ecab
   COOKIE_SECURE=true
   TRUST_PROXY=true
   RUST_LOG=ecab=info,tower_http=info
   ```

   Les autres réglages ont une valeur par défaut dans l'image (port 8080, dossiers, Chromium).
4. **Mounts** : un *bind mount* du dossier hôte `/srv/ecab/uploads` vers **`/app/uploads`**.
   Un dossier hôte (plutôt qu'un volume Docker) facilite la copie des 23 Go de fichiers.
   Sur le serveur, une fois : `mkdir -p /srv/ecab/uploads && chown -R 10001:10001 /srv/ecab/uploads`
   (10001 = utilisateur de l'application dans l'image).
5. **Domains** : le domaine voulu, **port 8080**, HTTPS avec Let's Encrypt.
   Pour la recette, un sous-domaine de test (ex. `ecab2.mds-dsi.org`) ; `ecab.mds-dsi.org` le jour J.
6. **Health check** (Advanced) : chemin `/health`, port 8080.
7. **Deploy**, puis suivre l'onglet Logs : le serveur applique les migrations au démarrage et
   affiche `ECAB-MS en écoute sur http://0.0.0.0:8080`.

## 3. Données (répétition, puis le jour J)

Le plus simple et le plus sûr : charger le dump dans une base MariaDB **temporaire** de Dokploy,
importer, puis supprimer cette base.

1. Sur le serveur, faire le dump de la base actuelle :
   `mysqldump -u mdsd_ecabms -p --single-transaction --no-tablespaces --default-character-set=utf8mb4 mdsd_ecabms | gzip > /root/ecab-db.sql.gz`
2. Dokploy : **Create Service → Database → MariaDB** (10.11), nommée par exemple `ecab-source`.
   Y charger le dump : `gunzip -c ecab-db.sql.gz | docker exec -i <conteneur-mariadb> mariadb -uroot -p<mdp> <base>`.
3. Dans le terminal du conteneur de l'application (onglet Docker → Terminal) :

   ```bash
   SOURCE_DATABASE_URL='mysql://root:MDP@ADRESSE_INTERNE_MARIADB:3306/BASE' /app/import --reset
   ```

   L'outil copie les 15 tables, recale les compteurs, pose les profils des services et **échoue en
   cas d'écart de comptage**. `--reset` vide d'abord la base cible : ne jamais le lancer une fois la
   nouvelle application en service.
4. Supprimer la base `ecab-source` quand l'import est validé.

## 4. Fichiers joints (23 Go)

Les fichiers sont déjà sur ce serveur, dans `/home/mds-dsi.org/ecab.mds-dsi.org/public/uploads/`
(sous-dossiers `courrier_arrives`, `courrier_sortis`, `imputations`, `cr`, `mission`, `avatar`).

**Pendant la recette, sans rien copier** : monter ce dossier **en lecture seule** sur `/app/uploads`
(bind mount, option *read only*). La nouvelle application lit les vrais fichiers, sans pouvoir les
modifier. Les fichiers doivent être lisibles par l'utilisateur 10001 de l'image : vérifier avec
`ls -ln` (en général lisibles par tous). Dans ce mode, l'envoi de nouveaux fichiers échoue : c'est
voulu, la recette ne doit pas écrire dans les fichiers de production.

**Pour la mise en service**, copier vers un dossier propre à la nouvelle application (le disque a
la place : 52 Go libres pour 23 Go) :

```bash
rsync -a --info=progress2 /home/mds-dsi.org/ecab.mds-dsi.org/public/uploads/ /srv/ecab/uploads/
```

Puis `chown -R 10001:10001 /srv/ecab/uploads` et remplacer le montage en lecture seule par
`/srv/ecab/uploads` → `/app/uploads` (lecture-écriture). Faire une première copie quelques jours
avant la bascule ; le jour J, relancer la même commande ne copie que les nouveaux fichiers.

## 5. Recette

Sur le sous-domaine de test, avec une copie des données : un utilisateur du service courrier, le
directeur de cabinet ou son équipe, un service ordinaire, l'accueil et un chargé d'études déroulent
leurs tâches habituelles. Faire valider les écarts listés dans `docs/regles-*.md`.

## 6. Bascule (jour J)

1. Page de maintenance sur l'ancienne application (plus aucune saisie).
2. Dernier dump → import (`--reset`) → vérification des comptages.
3. Dernier `rsync` des fichiers joints.
4. Dans Traefik / Dokploy : retirer `ecab.mds-dsi.org` de la route vers OpenLiteSpeed et l'ajouter
   aux domaines de l'application (le DNS ne change pas, c'est le même serveur).
5. Contrôles : connexion, une liste par module, ouverture d'un PDF joint, une fiche de suivi.
6. Garder l'ancienne application et son dump intacts quelques jours pour pouvoir revenir en arrière.
7. Après la bascule : changer les secrets de l'ancien code (mot de passe Gmail, clés Twilio, SMTP).
