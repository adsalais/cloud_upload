# intake — service de gestion (prototype offline)

Intake discret de données d'incident (DFIR) sur stockage compatible S3, prototypé
hors-ligne avec MinIO. Voir la conception dans `docs/superpowers/specs/`.

## Prérequis
- **Docker** (fait tourner MinIO *et* le client `mc` — aucune install hôte de `mc`).
- **Rust** (stable).
- **Node ≥ 20** (uniquement pour les tests du site — `node --test`, sans npm).

## Démarrage
```bash
docker compose up -d
source config.example.env          # exporte les variables INTAKE_* + met bin/ sur le PATH
bash scripts/bootstrap-minio.sh    # attend MinIO + valide le wrapper mc (Docker)
cargo build --release
```

> `bin/mc` est un wrapper qui exécute l'image officielle `minio/mc` via Docker
> (`--network host`, auth sans état via `MC_HOST_<alias>`). `source config.example.env`
> ajoute `bin/` au PATH pour que `mc` y soit résolu.

## Cycle d'une affaire
```bash
./target/release/intake create-case acme-2026     # crée buckets/site/clé, imprime l'URL + creds
./target/release/intake pull-case  acme-2026 --dest ./pulled
./target/release/intake teardown-case acme-2026 --yes
```

## Tests
```bash
# Rust (nécessite MinIO lancé + bin/ sur le PATH pour les tests qui utilisent mc) :
source config.example.env
cargo test

# Site (zéro dépendance, runner intégré de Node) :
eval "$(bash site-tests/provision.sh)"
node --test site-tests/
bash site-tests/deprovision.sh
```

## Structure
- `crates/core` — bibliothèque : plan de données S3 (portable) + plan de contrôle IAM MinIO.
- `crates/cli` — binaire `intake` (façade CLI ; un binaire `web` interne pourra la remplacer).
- `site/` — site d'upload zéro-dépendance (SigV4 Web Crypto + multipart).
- `bin/mc` — wrapper Docker pour le client MinIO.
