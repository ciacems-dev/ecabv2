# Image de production ECAB-MS (Dokploy).
#   docker build -t ecab .
# Deux binaires : `ecab` (serveur web) et `import` (migration des données MariaDB → PostgreSQL).

# --- Compilation --------------------------------------------------------------------------
FROM rust:1.95-slim-bookworm AS build
WORKDIR /app

# Dépendances d'abord : couche mise en cache tant que Cargo.toml / Cargo.lock ne changent pas.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/bin \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > src/bin/import.rs \
    && cargo build --release --locked \
    && rm -rf src

# Code de l'application (les migrations SQL sont intégrées au binaire à la compilation).
COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs src/bin/import.rs && cargo build --release --locked --bins

# --- Exécution ----------------------------------------------------------------------------
FROM debian:bookworm-slim

# Chromium : fiche de suivi PDF. fonts-liberation : métriques identiques à Arial (police de la fiche).
RUN apt-get update \
    && apt-get install -y --no-install-recommends chromium fonts-liberation fonts-dejavu-core ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Utilisateur sans privilèges.
RUN useradd --system --uid 10001 --home-dir /app ecab
WORKDIR /app

COPY --from=build /app/target/release/ecab /app/target/release/import /app/
COPY templates ./templates
COPY static ./static
RUN mkdir -p /app/uploads && chown ecab:ecab /app/uploads

USER ecab
ENV BIND_ADDR=0.0.0.0:8080 \
    TEMPLATES_DIR=/app/templates \
    STATIC_DIR=/app/static \
    UPLOADS_DIR=/app/uploads \
    CHROME_BIN=chromium \
    HOME=/tmp \
    RUST_LOG=ecab=info,tower_http=info

EXPOSE 8080
# Les fichiers joints (23 Go) vivent dans un volume monté sur /app/uploads.
VOLUME ["/app/uploads"]
CMD ["/app/ecab"]
