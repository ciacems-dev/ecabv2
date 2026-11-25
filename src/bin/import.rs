//! Import des données MariaDB (ancienne application Symfony) vers PostgreSQL.
//!
//! Usage :
//!   SOURCE_DATABASE_URL=mysql://root:root@127.0.0.1:3307/mdsd_ecabms \
//!   DATABASE_URL=postgres://ecab:ecab@127.0.0.1:5433/ecab \
//!   cargo run --bin import -- [--reset]
//!
//! - applique les migrations sur la base cible ;
//! - `--reset` vide d'abord les tables cibles (pour les répétitions de migration) ;
//! - copie chaque table dans l'ordre des clés étrangères, colonnes explicites ;
//! - recale les séquences d'identifiants ;
//! - compare le nombre de lignes source / cible et échoue en cas d'écart.

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::{MySqlPool, PgPool, Postgres, QueryBuilder, Row};

#[derive(Clone, Copy)]
enum Kind {
    Int,
    Text,
    Bool,
    Date,
    Timestamp,
    Time,
    /// Texte JSON côté MariaDB → jsonb côté PostgreSQL (`user.roles`)
    Json,
}

struct Table {
    source: &'static str,
    target: &'static str,
    columns: &'static [(&'static str, Kind)],
}

use Kind::*;

/// Tables dans l'ordre des dépendances. `messenger_messages` et
/// `doctrine_migration_versions` sont volontairement ignorées (données techniques Symfony).
const TABLES: &[Table] = &[
    Table { source: "zone", target: "zone", columns: &[("id", Int), ("codezone", Text), ("libzone", Text)] },
    Table {
        source: "pays",
        target: "pays",
        columns: &[
            ("id", Int), ("zone_id", Int), ("nompays", Text), ("tva", Text), ("capital", Text),
            ("monnaie_code", Text), ("monnaie_nom", Text), ("indicatif", Text), ("iso_code2", Text),
            ("iso_code3", Text), ("iso_number", Text), ("ltd", Text),
        ],
    },
    Table { source: "destinataire", target: "destinataire", columns: &[("id", Int), ("libelle", Text), ("statut", Bool)] },
    Table {
        source: "user",
        target: "users",
        columns: &[
            ("id", Int), ("pays_id", Int), ("destinataire_id", Int), ("email", Text), ("roles", Json),
            ("password", Text), ("is_verified", Bool), ("nom", Text), ("prenom", Text), ("civilite", Text),
            ("telephone", Text), ("active", Bool),
        ],
    },
    Table {
        source: "loginattemp",
        target: "loginattemp",
        columns: &[("id", Int), ("user_id", Int), ("ip", Text), ("date_start", Timestamp), ("date_end", Timestamp)],
    },
    Table {
        source: "carrive",
        target: "carrive",
        columns: &[
            ("id", Int), ("destinataire_id", Int), ("datearrive", Timestamp), ("numero", Int), ("datecour", Date),
            ("numcour", Int), ("expeditaire", Text), ("objet", Text), ("courrier", Text), ("create_at", Timestamp),
            ("date", Date), ("archive", Bool), ("confi", Bool), ("deposant", Text), ("action", Bool),
        ],
    },
    Table {
        source: "document",
        target: "document",
        columns: &[("id", Int), ("arrive_id", Int), ("libelle", Text), ("createat", Timestamp), ("fichier", Text)],
    },
    Table {
        source: "imputation",
        target: "imputation",
        columns: &[
            ("id", Int), ("courrier_id", Int), ("destinataire_id", Int), ("responsable_id", Int), ("date", Date),
            ("delais", Date), ("reponse", Text), ("statut", Bool), ("commentaire", Text), ("etat", Text),
            ("doctoure", Text), ("dateretour", Timestamp), ("observationdir", Text), ("dateobs", Timestamp),
            ("docfinal", Text), ("datefinale", Timestamp), ("effet", Text), ("archive", Bool), ("transmet", Bool),
        ],
    },
    Table { source: "typedepart", target: "typedepart", columns: &[("id", Int), ("libelle", Text), ("statut", Bool)] },
    Table {
        source: "csortir",
        target: "csortir",
        columns: &[
            ("id", Int), ("auteur_id", Int), ("type_id", Int), ("datesortir", Timestamp), ("numsortir", Int),
            ("objet", Text), ("destinataire", Text), ("courrier", Text), ("decharge", Text), ("update_at", Timestamp),
            ("agent", Text), ("statut", Bool), ("date", Date), ("archive", Bool), ("initiateur_id", Int),
        ],
    },
    Table {
        source: "compterendu",
        target: "compterendu",
        columns: &[
            ("id", Int), ("date", Date), ("time", Text), ("debut", Time), ("fin", Time), ("lieu", Text),
            ("presidence", Text), ("cr", Text), ("listep", Text), ("create_at", Timestamp), ("redacteur", Text),
            ("archive", Bool), ("objet", Text), ("statut", Bool),
        ],
    },
    Table {
        source: "diligence",
        target: "diligence",
        columns: &[
            ("id", Int), ("cr_id", Int), ("titre", Text), ("libelle", Text), ("ressource", Text), ("delai", Date),
            ("debut", Date), ("etat", Bool), ("observation", Text), ("create_at", Timestamp), ("update_at", Date),
            ("archive", Bool), ("taux", Int),
        ],
    },
    Table {
        source: "mission",
        target: "mission",
        columns: &[
            ("id", Int), ("destinataire_id", Int), ("objet", Text), ("budget", Text), ("transport", Text),
            ("destination", Text), ("datedebut", Date), ("datefin", Date), ("participant", Text), ("matricule", Text),
            ("emploi", Text), ("fonction", Text), ("ordrem", Text), ("rapportm", Text), ("statutr", Bool),
            ("lieu", Text), ("numero", Int), ("dateenregistre", Date),
        ],
    },
    Table {
        source: "audience",
        target: "audience",
        columns: &[
            ("id", Int), ("destinataire_id", Int), ("date", Date), ("heure", Time), ("nom", Text), ("service", Text),
            ("telephone", Text), ("rdv", Bool), ("objet", Text), ("qualite", Text), ("statut", Bool),
            ("commentaire", Text), ("email", Text),
        ],
    },
    Table {
        source: "rendvous",
        target: "rendvous",
        columns: &[
            ("id", Int), ("audience_id", Int), ("date", Date), ("heured", Time), ("heuref", Time), ("lieu", Text),
            ("datefin", Date), ("message", Text),
        ],
    },
];

/// Valeur lue côté MariaDB, prête à être liée côté PostgreSQL.
enum Value {
    Int(Option<i32>),
    Text(Option<String>),
    Bool(Option<bool>),
    Date(Option<NaiveDate>),
    Timestamp(Option<NaiveDateTime>),
    Time(Option<NaiveTime>),
    Json(Option<serde_json::Value>),
}

/// Lit une colonne texte. MariaDB annonce certaines colonnes (ex. JSON `longtext`) comme binaires :
/// dans ce cas on lit les octets et on les décode en UTF-8.
fn read_text(row: &sqlx::mysql::MySqlRow, col: &str) -> Result<Option<String>> {
    match row.try_get::<Option<String>, _>(col) {
        Ok(v) => Ok(v),
        Err(_) => {
            let bytes: Option<Vec<u8>> = row.try_get(col)?;
            Ok(bytes.map(String::from_utf8).transpose()?)
        }
    }
}

fn read_value(row: &sqlx::mysql::MySqlRow, col: &str, kind: Kind, table: &str) -> Result<Value> {
    let ctx = || format!("lecture de {table}.{col}");
    Ok(match kind {
        Int => Value::Int(row.try_get(col).with_context(ctx)?),
        Text => Value::Text(read_text(row, col).with_context(ctx)?),
        Bool => {
            // tinyint(1) : lu comme entier pour accepter toute valeur, 0 = false, sinon true
            let v: Option<i8> = row.try_get(col).with_context(ctx)?;
            Value::Bool(v.map(|n| n != 0))
        }
        Date => Value::Date(row.try_get(col).with_context(ctx)?),
        Timestamp => Value::Timestamp(row.try_get(col).with_context(ctx)?),
        Time => Value::Time(row.try_get(col).with_context(ctx)?),
        Json => {
            let raw = read_text(row, col).with_context(ctx)?;
            let parsed = raw
                .map(|s| serde_json::from_str(&s).with_context(|| format!("JSON invalide dans {table}.{col} : {s}")))
                .transpose()?
                // PHP sérialise parfois un tableau réindexé en objet ({"1":"ROLE_ADMIN"}) :
                // Symfony le lisait comme la liste de ses valeurs, on le normalise ainsi.
                .map(|v| match v {
                    serde_json::Value::Object(map) => serde_json::Value::Array(map.into_values().collect()),
                    other => other,
                });
            Value::Json(parsed)
        }
    })
}

async fn copy_table(src: &MySqlPool, dst: &PgPool, t: &Table) -> Result<u64> {
    let cols: Vec<&str> = t.columns.iter().map(|(c, _)| *c).collect();
    let select = format!(
        "SELECT {} FROM `{}` ORDER BY id",
        cols.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "),
        t.source
    );
    let rows = sqlx::query(&select).fetch_all(src).await.with_context(|| format!("lecture de {}", t.source))?;

    let mut tx = dst.begin().await?;
    // Insertion par lots pour rester sous la limite de paramètres PostgreSQL (65 535).
    let batch_size = 60_000 / cols.len();
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut values = Vec::with_capacity(chunk.len());
        for row in chunk {
            let mut v = Vec::with_capacity(cols.len());
            for (col, kind) in t.columns {
                v.push(read_value(row, col, *kind, t.source)?);
            }
            values.push(v);
        }
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "INSERT INTO {} ({}) ",
            t.target,
            cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ")
        ));
        qb.push_values(values, |mut b, row| {
            for v in row {
                match v {
                    Value::Int(x) => b.push_bind(x),
                    Value::Text(x) => b.push_bind(x),
                    Value::Bool(x) => b.push_bind(x),
                    Value::Date(x) => b.push_bind(x),
                    Value::Timestamp(x) => b.push_bind(x),
                    Value::Time(x) => b.push_bind(x),
                    Value::Json(x) => b.push_bind(x),
                };
            }
        });
        qb.build().execute(&mut *tx).await.with_context(|| format!("écriture dans {}", t.target))?;
    }
    // Recale la séquence d'identité sur le plus grand id importé.
    sqlx::query(&format!(
        "SELECT setval(pg_get_serial_sequence('{0}', 'id'), COALESCE((SELECT MAX(id) FROM {0}), 0) + 1, false)",
        t.target
    ))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(rows.len() as u64)
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let reset = std::env::args().any(|a| a == "--reset");
    let src_url = std::env::var("SOURCE_DATABASE_URL").context("SOURCE_DATABASE_URL manquant (base MariaDB source)")?;
    let dst_url = std::env::var("DATABASE_URL").context("DATABASE_URL manquant (base PostgreSQL cible)")?;

    let src = MySqlPoolOptions::new().max_connections(2).connect(&src_url).await.context("connexion MariaDB")?;
    let dst = PgPoolOptions::new().max_connections(2).connect(&dst_url).await.context("connexion PostgreSQL")?;

    sqlx::migrate!("./migrations").run(&dst).await.context("migrations PostgreSQL")?;

    if reset {
        let names: Vec<&str> = TABLES.iter().map(|t| t.target).collect();
        sqlx::query(&format!("TRUNCATE {} RESTART IDENTITY CASCADE", names.join(", "))).execute(&dst).await?;
        println!("Tables cibles vidées.");
    } else {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&dst).await?;
        if n > 0 {
            bail!("La base cible contient déjà des données. Relancez avec --reset pour les remplacer.");
        }
    }

    let mut errors = Vec::new();
    println!("{:<15} {:>8} {:>8}", "table", "source", "cible");
    for t in TABLES {
        let copied = copy_table(&src, &dst, t).await?;
        let src_count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM `{}`", t.source)).fetch_one(&src).await?;
        let dst_count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", t.target)).fetch_one(&dst).await?;
        let ok = src_count == dst_count && copied as i64 == src_count;
        println!("{:<15} {:>8} {:>8} {}", t.target, src_count, dst_count, if ok { "ok" } else { "ÉCART" });
        if !ok {
            errors.push(t.target);
        }
    }

    if !errors.is_empty() {
        bail!("Écarts de comptage sur : {}", errors.join(", "));
    }
    println!("Import terminé, tous les comptages concordent.");
    Ok(())
}
