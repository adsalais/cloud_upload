# Plan A — Service de gestion (intake DFIR) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un service Rust (bibliothèque `intake-core` + binaire CLI `intake`) qui crée, alimente, récupère et détruit des affaires d'intake d'incident sur un stockage compatible S3, prototypé hors-ligne avec MinIO.

**Architecture:** Cœur métier réutilisable (`intake-core`) exposant un plan de données S3 portable (`aws-sdk-s3`, path-style) et un plan de contrôle IAM spécifique MinIO (via le CLI `mc`). Le binaire `intake` (clap) est une façade fine ; un futur binaire `web` réutilisera le même cœur. Deux buckets par affaire : données privées (versioning + clé scopée écriture-seule) et site public.

**Tech Stack:** Rust 2021, `aws-sdk-s3` v1, `aws-config` v1, `tokio`, `clap` v4, `serde`/`serde_json`, `anyhow`, `getrandom` ; MinIO (Docker) ; `mc` (MinIO Client) sur l'hôte.

## Global Constraints

- Rust edition **2021** ; deux crates dans un workspace : `crates/core` (lib `intake-core`), `crates/cli` (bin `intake`).
- S3 en **path-style** (`force_path_style(true)`), région **`us-east-1`** (défaut MinIO ; ne pas envoyer de `CreateBucketConfiguration`).
- Config via **variables d'environnement** (préfixe `INTAKE_`), défauts pointant sur MinIO local ; **aucun secret dans un fichier commité**.
- **`Cargo.lock` est commité** (application binaire).
- Nom de bucket **DNS-compatible** : minuscules, `intake-data-<id>-<rand>` / `intake-site-<id>-<rand>`, `<id>` slugifié.
- Le bucket **data** ne reçoit **jamais** de policy publique. La clé scopée n'autorise que l'écriture (`PutObject` + multipart) sur le bucket data.
- `mc` est configuré côté hôte avec un alias (défaut `myminio`) ; les tests d'intégration supposent MinIO lancé et l'alias posé (Task 1).

---

### Task 1 : Environnement MinIO + squelette du workspace + config

**Files:**
- Create: `docker-compose.yml`
- Create: `bin/mc` (wrapper : exécute `mc` via l'image Docker `minio/mc`, zéro install hôte)
- Create: `scripts/bootstrap-minio.sh`
- Create: `config.example.env`

> **Adaptation docker-mc :** pas d'install hôte de `mc`. `bin/mc` lance l'image
> officielle `minio/mc` avec `--network host`, une auth sans état via
> `MC_HOST_<alias>` et `-v /tmp:/tmp` (+ `-v $PWD`) pour les fichiers `--policy`.
> Le code Rust et les scripts appellent une commande nommée `mc` ; il suffit d'avoir
> `bin/` sur le PATH (`export PATH="$PWD/bin:$PATH"`). Le reste du plan est inchangé.
- Create: `Cargo.toml` (workspace)
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/core/src/config.rs`
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Test: `crates/core/tests/connectivity.rs`

**Interfaces:**
- Produces: `intake_core::config::Config` avec champs `endpoint, region, admin_access_key, admin_secret_key, mc_alias, mc_parent, site_dir, cases_dir: String` et `Config::from_env() -> Config`.
- Produces: `intake_core::s3_dataplane::build_client(cfg: &Config, access: &str, secret: &str, session: Option<&str>) -> aws_sdk_s3::Client`.

- [ ] **Step 1 : Écrire `docker-compose.yml`**

```yaml
services:
  minio:
    image: minio/minio:latest
    command: server /data --console-address ":9001"
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    ports:
      - "9000:9000"
      - "9001:9001"
    volumes:
      - ./.minio-data:/data
```

- [ ] **Step 2a : Écrire le wrapper `bin/mc`** (client MinIO via Docker — voir le fichier
  livré). Le rendre exécutable : `chmod +x bin/mc`.

- [ ] **Step 2b : Écrire `scripts/bootstrap-minio.sh` (attend MinIO + valide le wrapper mc)**

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$ROOT/bin:$PATH"
ALIAS="${INTAKE_MC_ALIAS:-myminio}"
ENDPOINT="${INTAKE_ENDPOINT:-http://localhost:9000}"
echo "Attente de MinIO sur $ENDPOINT ..."
for _ in $(seq 1 60); do
  curl -fsS "$ENDPOINT/minio/health/live" >/dev/null 2>&1 && { echo "MinIO en ligne."; break; }
  sleep 1
done
mc admin info "$ALIAS" >/dev/null   # valide le wrapper mc (Docker) et pré-tire l'image
echo "mc (Docker) opérationnel — alias '$ALIAS' via MC_HOST."
```

- [ ] **Step 3 : Écrire `config.example.env` (documentation des variables)**

```bash
# Copier/adapter puis `source` avant de lancer le CLI ou les tests.
export PATH="$PWD/bin:$PATH"   # rend `mc` (wrapper Docker) disponible
export INTAKE_ENDPOINT=http://localhost:9000
export INTAKE_REGION=us-east-1
export INTAKE_ADMIN_ACCESS_KEY=minioadmin
export INTAKE_ADMIN_SECRET_KEY=minioadmin
export INTAKE_MC_ALIAS=myminio
export INTAKE_MC_PARENT=minioadmin
export INTAKE_SITE_DIR=./site
export INTAKE_CASES_DIR=./cases
```

- [ ] **Step 4 : Écrire le workspace `Cargo.toml`**

```toml
[workspace]
members = ["crates/core", "crates/cli"]
resolver = "2"
```

- [ ] **Step 5 : Écrire `crates/core/Cargo.toml`**

```toml
[package]
name = "intake-core"
version = "0.1.0"
edition = "2021"

[dependencies]
aws-config = "1"
aws-sdk-s3 = "1"
aws-credential-types = "1"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
getrandom = "0.2"
```

- [ ] **Step 6 : Écrire `crates/core/src/config.rs`**

```rust
#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: String,
    pub region: String,
    pub admin_access_key: String,
    pub admin_secret_key: String,
    pub mc_alias: String,
    pub mc_parent: String,
    pub site_dir: String,
    pub cases_dir: String,
}

impl Config {
    pub fn from_env() -> Self {
        fn v(k: &str, d: &str) -> String {
            std::env::var(k).unwrap_or_else(|_| d.to_string())
        }
        Config {
            endpoint: v("INTAKE_ENDPOINT", "http://localhost:9000"),
            region: v("INTAKE_REGION", "us-east-1"),
            admin_access_key: v("INTAKE_ADMIN_ACCESS_KEY", "minioadmin"),
            admin_secret_key: v("INTAKE_ADMIN_SECRET_KEY", "minioadmin"),
            mc_alias: v("INTAKE_MC_ALIAS", "myminio"),
            mc_parent: v("INTAKE_MC_PARENT", "minioadmin"),
            site_dir: v("INTAKE_SITE_DIR", "./site"),
            cases_dir: v("INTAKE_CASES_DIR", "./cases"),
        }
    }
}
```

- [ ] **Step 7 : Écrire le début de `crates/core/src/s3_dataplane.rs` (constructeur de client)**

```rust
use crate::config::Config;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;

pub fn build_client(cfg: &Config, access: &str, secret: &str, session: Option<&str>) -> Client {
    let creds = Credentials::new(
        access,
        secret,
        session.map(|s| s.to_string()),
        None,
        "static",
    );
    let s3conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(cfg.region.clone()))
        .endpoint_url(cfg.endpoint.clone())
        .credentials_provider(creds)
        .force_path_style(true)
        .build();
    Client::from_conf(s3conf)
}
```

- [ ] **Step 8 : Écrire `crates/core/src/lib.rs`**

```rust
pub mod case;
pub mod config;
pub mod minio_iam;
pub mod ops;
pub mod s3_dataplane;
```

- [ ] **Step 9 : Créer les stubs de modules pour que ça compile** (`case.rs`, `minio_iam.rs`, `ops.rs` vides pour l'instant)

```rust
// crates/core/src/case.rs, minio_iam.rs, ops.rs : chacun contient juste :
// (contenu réel ajouté dans les tâches suivantes)
```

Écrire dans chacun des trois fichiers une ligne de commentaire :
```rust
//! Rempli dans une tâche ultérieure.
```

- [ ] **Step 10 : Écrire `crates/cli/Cargo.toml` et un `main.rs` minimal**

`crates/cli/Cargo.toml` :
```toml
[package]
name = "intake-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "intake"
path = "src/main.rs"

[dependencies]
intake-core = { path = "../core" }
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

`crates/cli/src/main.rs` :
```rust
fn main() {
    println!("intake — service de gestion (voir sous-commandes ajoutées en Task 7)");
}
```

- [ ] **Step 11 : Écrire le test de connectivité `crates/core/tests/connectivity.rs`**

```rust
use intake_core::config::Config;
use intake_core::s3_dataplane::build_client;

#[tokio::test]
async fn connects_and_lists_buckets() {
    let cfg = Config::from_env();
    let client = build_client(&cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None);
    let res = client.list_buckets().send().await;
    assert!(res.is_ok(), "list_buckets a échoué : {res:?}");
}
```

- [ ] **Step 12 : Lancer l'environnement et vérifier que le test passe**

```bash
docker compose up -d
bash scripts/bootstrap-minio.sh
cargo test -p intake-core --test connectivity
```
Expected: PASS (MinIO répond, credentials admin OK).

- [ ] **Step 13 : Commit**

```bash
git add docker-compose.yml scripts/ config.example.env Cargo.toml Cargo.lock crates/
git commit -m "chore: MinIO env + Rust workspace scaffold + S3 client + connectivity test"
```

---

### Task 2 : Plan de données S3 — cycle de vie des buckets

**Files:**
- Modify: `crates/core/src/s3_dataplane.rs`
- Test: `crates/core/tests/bucket_lifecycle.rs`

**Interfaces:**
- Produces: `pub struct S3DataPlane { pub client: Client }`, `S3DataPlane::new(Client) -> Self`.
- Produces méthodes `async` (retour `anyhow::Result<()>` sauf indication) :
  - `create_bucket(&self, name: &str)`
  - `enable_versioning(&self, bucket: &str)`
  - `set_cors(&self, bucket: &str, origin: &str)`
  - `delete_bucket(&self, bucket: &str)` (purge d'abord toutes les versions et delete markers)

- [ ] **Step 1 : Écrire le test `crates/core/tests/bucket_lifecycle.rs`**

```rust
use intake_core::config::Config;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

fn dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

#[tokio::test]
async fn create_versioned_bucket_then_delete_purges_versions() {
    let cfg = Config::from_env();
    let dp = dp(&cfg);
    let bucket = "intake-test-lifecycle-0001";

    // nettoyage préventif si un run précédent a échoué
    let _ = dp.delete_bucket(bucket).await;

    dp.create_bucket(bucket).await.expect("create_bucket");
    dp.enable_versioning(bucket).await.expect("enable_versioning");
    dp.set_cors(bucket, "*").await.expect("set_cors");

    // écrire deux versions du même objet
    for body in ["v1", "v2"] {
        dp.client
            .put_object()
            .bucket(bucket)
            .key("obj.txt")
            .body(body.as_bytes().to_vec().into())
            .send()
            .await
            .expect("put_object");
    }

    // versioning actif => au moins deux versions listées
    let versions = dp
        .client
        .list_object_versions()
        .bucket(bucket)
        .send()
        .await
        .expect("list_object_versions");
    assert!(versions.versions().len() >= 2, "versioning inactif ?");

    // delete_bucket doit purger versions + supprimer le bucket
    dp.delete_bucket(bucket).await.expect("delete_bucket");
    let head = dp.client.head_bucket().bucket(bucket).send().await;
    assert!(head.is_err(), "le bucket existe encore après delete_bucket");
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-core --test bucket_lifecycle`
Expected: FAIL à la compilation (`S3DataPlane` / méthodes inexistantes).

- [ ] **Step 3 : Implémenter dans `crates/core/src/s3_dataplane.rs`**

Ajouter en bas du fichier :

```rust
use aws_sdk_s3::types::{
    BucketVersioningStatus, CorsConfiguration, CorsRule, VersioningConfiguration,
};

pub struct S3DataPlane {
    pub client: Client,
}

impl S3DataPlane {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn create_bucket(&self, name: &str) -> anyhow::Result<()> {
        // région us-east-1 : pas de CreateBucketConfiguration
        self.client.create_bucket().bucket(name).send().await?;
        Ok(())
    }

    pub async fn enable_versioning(&self, bucket: &str) -> anyhow::Result<()> {
        let vc = VersioningConfiguration::builder()
            .status(BucketVersioningStatus::Enabled)
            .build();
        self.client
            .put_bucket_versioning()
            .bucket(bucket)
            .versioning_configuration(vc)
            .send()
            .await?;
        Ok(())
    }

    pub async fn set_cors(&self, bucket: &str, origin: &str) -> anyhow::Result<()> {
        let rule = CorsRule::builder()
            .allowed_methods("PUT")
            .allowed_methods("POST")
            .allowed_methods("GET")
            .allowed_methods("HEAD")
            .allowed_origins(origin)
            .allowed_headers("*")
            .expose_headers("ETag")
            .max_age_seconds(3000)
            .build()?;
        let cc = CorsConfiguration::builder().cors_rules(rule).build()?;
        self.client
            .put_bucket_cors()
            .bucket(bucket)
            .cors_configuration(cc)
            .send()
            .await?;
        Ok(())
    }

    pub async fn delete_bucket(&self, bucket: &str) -> anyhow::Result<()> {
        let mut key_marker: Option<String> = None;
        let mut ver_marker: Option<String> = None;
        loop {
            let mut req = self.client.list_object_versions().bucket(bucket);
            if let Some(k) = &key_marker {
                req = req.key_marker(k);
            }
            if let Some(v) = &ver_marker {
                req = req.version_id_marker(v);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(_) => break, // bucket inexistant : rien à purger
            };
            for v in resp.versions() {
                if let (Some(k), Some(id)) = (v.key(), v.version_id()) {
                    self.client
                        .delete_object()
                        .bucket(bucket)
                        .key(k)
                        .version_id(id)
                        .send()
                        .await?;
                }
            }
            for d in resp.delete_markers() {
                if let (Some(k), Some(id)) = (d.key(), d.version_id()) {
                    self.client
                        .delete_object()
                        .bucket(bucket)
                        .key(k)
                        .version_id(id)
                        .send()
                        .await?;
                }
            }
            if resp.is_truncated().unwrap_or(false) {
                key_marker = resp.next_key_marker().map(str::to_string);
                ver_marker = resp.next_version_id_marker().map(str::to_string);
            } else {
                break;
            }
        }
        self.client.delete_bucket().bucket(bucket).send().await?;
        Ok(())
    }
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-core --test bucket_lifecycle`
Expected: PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/s3_dataplane.rs crates/core/tests/bucket_lifecycle.rs Cargo.lock
git commit -m "feat(core): S3 bucket lifecycle (create/versioning/cors/delete-purge)"
```

---

### Task 3 : Plan de données S3 — publication du site + I/O objets

**Files:**
- Modify: `crates/core/src/s3_dataplane.rs`
- Test: `crates/core/tests/site_and_objects.rs`

**Interfaces:**
- Produces méthodes sur `S3DataPlane` :
  - `put_public_read_policy(&self, bucket: &str)`
  - `put_object_bytes(&self, bucket: &str, key: &str, bytes: Vec<u8>, content_type: &str)`
  - `deploy_dir(&self, bucket: &str, dir: &std::path::Path, key_prefix: &str)`
  - `download_all(&self, bucket: &str, dest: &std::path::Path) -> anyhow::Result<Vec<(String, std::path::PathBuf)>>`

- [ ] **Step 1 : Écrire le test `crates/core/tests/site_and_objects.rs`**

```rust
use intake_core::config::Config;
use intake_core::s3_dataplane::{build_client, S3DataPlane};
use std::path::Path;

fn dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

#[tokio::test]
async fn deploy_public_site_then_download_objects() {
    let cfg = Config::from_env();
    let dp = dp(&cfg);
    let bucket = "intake-test-site-0001";
    let _ = dp.delete_bucket(bucket).await;
    dp.create_bucket(bucket).await.unwrap();
    dp.put_public_read_policy(bucket).await.unwrap();

    // déployer un dossier temporaire contenant index.html
    let tmp = std::env::temp_dir().join("intake-site-src");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("index.html"), b"<h1>ok</h1>").unwrap();
    dp.deploy_dir(bucket, &tmp, "site/").await.unwrap();

    // lecture publique (anonyme) via HTTP path-style
    let url = format!("{}/{}/site/index.html", cfg.endpoint, bucket);
    let body = reqwest_get(&url).await;
    assert!(body.contains("ok"), "index.html non lisible publiquement");

    // I/O objets : put + download_all
    dp.put_object_bytes(bucket, "data/a.bin", vec![1, 2, 3], "application/octet-stream")
        .await
        .unwrap();
    let dest = std::env::temp_dir().join("intake-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = dp.download_all(bucket, &dest).await.unwrap();
    assert!(files.iter().any(|(k, _)| k == "data/a.bin"));
    let got = std::fs::read(dest.join("data/a.bin")).unwrap();
    assert_eq!(got, vec![1, 2, 3]);

    dp.delete_bucket(bucket).await.unwrap();
}

// petit GET HTTP sans dépendance : on passe par `curl` (présent en dev)
async fn reqwest_get(url: &str) -> String {
    let out = tokio::process::Command::new("curl")
        .args(["-s", url])
        .output()
        .await
        .expect("curl");
    String::from_utf8_lossy(&out.stdout).to_string()
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-core --test site_and_objects`
Expected: FAIL à la compilation (méthodes inexistantes).

- [ ] **Step 3 : Implémenter dans `crates/core/src/s3_dataplane.rs`**

Ajouter :

```rust
use aws_sdk_s3::primitives::ByteStream;
use std::path::{Path, PathBuf};

impl S3DataPlane {
    pub async fn put_public_read_policy(&self, bucket: &str) -> anyhow::Result<()> {
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"AWS": ["*"]},
                "Action": ["s3:GetObject"],
                "Resource": [format!("arn:aws:s3:::{bucket}/*")]
            }]
        })
        .to_string();
        self.client
            .put_bucket_policy()
            .bucket(bucket)
            .policy(policy)
            .send()
            .await?;
        Ok(())
    }

    pub async fn put_object_bytes(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<()> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await?;
        Ok(())
    }

    pub async fn deploy_dir(
        &self,
        bucket: &str,
        dir: &Path,
        key_prefix: &str,
    ) -> anyhow::Result<()> {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let rel = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                let key = format!("{key_prefix}{rel}");
                let ct = content_type_for(&path);
                let body = ByteStream::from_path(&path).await?;
                self.client
                    .put_object()
                    .bucket(bucket)
                    .key(&key)
                    .content_type(ct)
                    .body(body)
                    .send()
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn download_all(
        &self,
        bucket: &str,
        dest: &Path,
    ) -> anyhow::Result<Vec<(String, PathBuf)>> {
        let mut out = Vec::new();
        let mut cont: Option<String> = None;
        loop {
            let mut req = self.client.list_objects_v2().bucket(bucket);
            if let Some(t) = &cont {
                req = req.continuation_token(t);
            }
            let resp = req.send().await?;
            for obj in resp.contents() {
                let key = match obj.key() {
                    Some(k) => k.to_string(),
                    None => continue,
                };
                let go = self.client.get_object().bucket(bucket).key(&key).send().await?;
                let data = go.body.collect().await?.into_bytes();
                let path = dest.join(&key);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, &data)?;
                out.push((key, path));
            }
            if resp.is_truncated().unwrap_or(false) {
                cont = resp.next_continuation_token().map(str::to_string);
            } else {
                break;
            }
        }
        Ok(out)
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-core --test site_and_objects`
Expected: PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/s3_dataplane.rs crates/core/tests/site_and_objects.rs Cargo.lock
git commit -m "feat(core): public site policy, deploy_dir, object download"
```

---

### Task 4 : Plan de contrôle IAM (MinIO) — clé scopée + tests de sécurité négatifs

**Files:**
- Modify: `crates/core/src/minio_iam.rs`
- Test: `crates/core/tests/scoped_key.rs`

**Interfaces:**
- Produces: `pub struct MinioIam { pub alias: String, pub parent: String }`, `MinioIam::from_config(&Config) -> Self`.
- Produces: `pub struct ScopedCreds { pub access_key: String, pub secret_key: String }`.
- Produces: `async fn create_scoped_upload_key(&self, data_bucket: &str) -> anyhow::Result<ScopedCreds>`.
- Produces: `async fn delete_scoped_key(&self, access_key: &str) -> anyhow::Result<()>`.

- [ ] **Step 1 : Écrire le test `crates/core/tests/scoped_key.rs` (positif + négatifs)**

```rust
use intake_core::config::Config;
use intake_core::minio_iam::MinioIam;
use intake_core::s3_dataplane::{build_client, S3DataPlane};

#[tokio::test]
async fn scoped_key_can_only_write_to_its_bucket() {
    let cfg = Config::from_env();
    let admin = S3DataPlane::new(build_client(
        &cfg,
        &cfg.admin_access_key,
        &cfg.admin_secret_key,
        None,
    ));
    let data_bucket = "intake-test-scoped-data-0001";
    let other_bucket = "intake-test-scoped-other-0001";
    let _ = admin.delete_bucket(data_bucket).await;
    let _ = admin.delete_bucket(other_bucket).await;
    admin.create_bucket(data_bucket).await.unwrap();
    admin.create_bucket(other_bucket).await.unwrap();

    let iam = MinioIam::from_config(&cfg);
    let creds = iam.create_scoped_upload_key(data_bucket).await.unwrap();

    // client construit avec la clé scopée
    let scoped = build_client(&cfg, &creds.access_key, &creds.secret_key, None);

    // POSITIF : écrire dans le bucket data
    let put_ok = scoped
        .put_object()
        .bucket(data_bucket)
        .key("evidence/a.bin")
        .body(vec![9u8, 9, 9].into())
        .send()
        .await;
    assert!(put_ok.is_ok(), "la clé scopée devrait pouvoir écrire : {put_ok:?}");

    // NÉGATIF 1 : lire l'objet -> refusé
    let get = scoped
        .get_object()
        .bucket(data_bucket)
        .key("evidence/a.bin")
        .send()
        .await;
    assert!(get.is_err(), "la clé scopée ne doit PAS pouvoir lire");

    // NÉGATIF 2 : lister -> refusé
    let list = scoped.list_objects_v2().bucket(data_bucket).send().await;
    assert!(list.is_err(), "la clé scopée ne doit PAS pouvoir lister");

    // NÉGATIF 3 : écrire dans un autre bucket -> refusé
    let put_other = scoped
        .put_object()
        .bucket(other_bucket)
        .key("x")
        .body(vec![1u8].into())
        .send()
        .await;
    assert!(put_other.is_err(), "la clé scopée ne doit écrire QUE dans son bucket");

    // révocation
    iam.delete_scoped_key(&creds.access_key).await.unwrap();

    // NÉGATIF 4 : après révocation, l'écriture échoue
    let put_after = scoped
        .put_object()
        .bucket(data_bucket)
        .key("evidence/b.bin")
        .body(vec![0u8].into())
        .send()
        .await;
    assert!(put_after.is_err(), "la clé révoquée ne doit plus fonctionner");

    admin.delete_bucket(data_bucket).await.unwrap();
    admin.delete_bucket(other_bucket).await.unwrap();
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-core --test scoped_key`
Expected: FAIL à la compilation (`MinioIam` inexistant).

- [ ] **Step 3 : Implémenter `crates/core/src/minio_iam.rs`**

```rust
use crate::config::Config;

pub struct MinioIam {
    pub alias: String,
    pub parent: String,
}

pub struct ScopedCreds {
    pub access_key: String,
    pub secret_key: String,
}

impl MinioIam {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            alias: cfg.mc_alias.clone(),
            parent: cfg.mc_parent.clone(),
        }
    }

    pub async fn create_scoped_upload_key(
        &self,
        data_bucket: &str,
    ) -> anyhow::Result<ScopedCreds> {
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Action": [
                    "s3:PutObject",
                    "s3:AbortMultipartUpload",
                    "s3:ListMultipartUploadParts"
                ],
                "Resource": [format!("arn:aws:s3:::{data_bucket}/*")]
            }]
        });
        let tmp = std::env::temp_dir().join(format!("intake-pol-{data_bucket}.json"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&policy)?)?;

        let out = tokio::process::Command::new("mc")
            .arg("--json")
            .args(["admin", "user", "svcacct", "add", "--policy"])
            .arg(&tmp)
            .arg(&self.alias)
            .arg(&self.parent)
            .output()
            .await?;
        let _ = std::fs::remove_file(&tmp);

        if !out.status.success() {
            anyhow::bail!(
                "mc svcacct add a échoué : {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        #[derive(serde::Deserialize)]
        struct Sa {
            #[serde(rename = "accessKey")]
            access_key: String,
            #[serde(rename = "secretKey")]
            secret_key: String,
        }
        let sa: Sa = serde_json::from_slice(&out.stdout)?;
        Ok(ScopedCreds {
            access_key: sa.access_key,
            secret_key: sa.secret_key,
        })
    }

    pub async fn delete_scoped_key(&self, access_key: &str) -> anyhow::Result<()> {
        let out = tokio::process::Command::new("mc")
            .args(["admin", "user", "svcacct", "rm", &self.alias, access_key])
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!(
                "mc svcacct rm a échoué : {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    }
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-core --test scoped_key`
Expected: PASS (positif OK ; 4 négatifs échouent bien).

Note : si MinIO répliquait les credentials de façon éventuellement cohérente, insérer un court `tokio::time::sleep(Duration::from_millis(500))` après création/révocation. Ne pas ajouter par défaut ; seulement en cas de flakiness observée.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/minio_iam.rs crates/core/tests/scoped_key.rs Cargo.lock
git commit -m "feat(core): MinIO scoped write-only key with negative security tests"
```

---

### Task 5 : Modèle `Case` + persistance JSON

**Files:**
- Modify: `crates/core/src/case.rs`
- Test: `crates/core/tests/case_persistence.rs`

**Interfaces:**
- Produces: `pub enum CaseState { Active, TornDown }` (serde).
- Produces: `pub struct Case { pub id, pub data_bucket, pub site_bucket, pub scoped_access_key, pub site_url: String, pub state: CaseState }` (tous `String` sauf `state`).
- Produces: `Case::save(&self, cases_dir: &str) -> anyhow::Result<()>` (écrit `<cases_dir>/<id>.json`).
- Produces: `Case::load(cases_dir: &str, id: &str) -> anyhow::Result<Case>`.

- [ ] **Step 1 : Écrire le test `crates/core/tests/case_persistence.rs`**

```rust
use intake_core::case::{Case, CaseState};

#[test]
fn save_then_load_roundtrip() {
    let dir = std::env::temp_dir().join("intake-cases-test");
    let _ = std::fs::remove_dir_all(&dir);
    let dir_s = dir.to_string_lossy().to_string();

    let c = Case {
        id: "acme-2026".into(),
        data_bucket: "intake-data-acme-2026-abcd".into(),
        site_bucket: "intake-site-acme-2026-abcd".into(),
        scoped_access_key: "AKIAEXAMPLE".into(),
        site_url: "http://localhost:9000/intake-site-acme-2026-abcd/site/index.html".into(),
        state: CaseState::Active,
    };
    c.save(&dir_s).unwrap();

    let loaded = Case::load(&dir_s, "acme-2026").unwrap();
    assert_eq!(loaded.data_bucket, c.data_bucket);
    assert_eq!(loaded.scoped_access_key, "AKIAEXAMPLE");
    assert!(matches!(loaded.state, CaseState::Active));
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-core --test case_persistence`
Expected: FAIL à la compilation.

- [ ] **Step 3 : Implémenter `crates/core/src/case.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CaseState {
    Active,
    TornDown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Case {
    pub id: String,
    pub data_bucket: String,
    pub site_bucket: String,
    pub scoped_access_key: String,
    pub site_url: String,
    pub state: CaseState,
}

impl Case {
    fn path(cases_dir: &str, id: &str) -> std::path::PathBuf {
        Path::new(cases_dir).join(format!("{id}.json"))
    }

    pub fn save(&self, cases_dir: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(cases_dir)?;
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(Self::path(cases_dir, &self.id), json)?;
        Ok(())
    }

    pub fn load(cases_dir: &str, id: &str) -> anyhow::Result<Case> {
        let bytes = std::fs::read(Self::path(cases_dir, id))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-core --test case_persistence`
Expected: PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/case.rs crates/core/tests/case_persistence.rs Cargo.lock
git commit -m "feat(core): Case model + JSON persistence"
```

---

### Task 6 : Orchestration `ops` — create / pull / teardown (bout-en-bout)

**Files:**
- Modify: `crates/core/src/ops.rs`
- Test: `crates/core/tests/end_to_end.rs`

**Interfaces:**
- Produces: `pub struct CreateResult { pub case: Case, pub scoped_secret_key: String }`.
- Produces: `async fn create_case(cfg: &Config, id: &str) -> anyhow::Result<CreateResult>`.
- Produces: `async fn pull_case(cfg: &Config, id: &str, dest: &std::path::Path) -> anyhow::Result<Vec<(String, std::path::PathBuf)>>`.
- Produces: `async fn teardown_case(cfg: &Config, id: &str) -> anyhow::Result<()>`.
- Consumes: `S3DataPlane` (Task 2/3), `MinioIam` (Task 4), `Case` (Task 5), `build_client` (Task 1).

- [ ] **Step 1 : Écrire le test `crates/core/tests/end_to_end.rs`**

```rust
use intake_core::config::Config;
use intake_core::ops;
use intake_core::s3_dataplane::build_client;

#[tokio::test]
async fn full_case_lifecycle() {
    let cfg = Config::from_env();
    let id = "e2e-0001";
    // au cas où un run précédent a laissé des traces
    let _ = ops::teardown_case(&cfg, id).await;

    let created = ops::create_case(&cfg, id).await.expect("create_case");
    assert!(created.case.data_bucket.starts_with("intake-data-e2e-0001-"));
    assert!(created.case.site_bucket.starts_with("intake-site-e2e-0001-"));

    // la victime (clé scopée) dépose une preuve
    let victim = build_client(
        &cfg,
        &created.case.scoped_access_key,
        &created.scoped_secret_key,
        None,
    );
    victim
        .put_object()
        .bucket(&created.case.data_bucket)
        .key("dump.raw")
        .body(vec![7u8; 1024].into())
        .send()
        .await
        .expect("upload victime");

    // l'équipe récupère
    let dest = std::env::temp_dir().join("intake-e2e-dl");
    let _ = std::fs::remove_dir_all(&dest);
    let files = ops::pull_case(&cfg, id, &dest).await.expect("pull_case");
    assert!(files.iter().any(|(k, _)| k == "dump.raw"));
    assert_eq!(std::fs::read(dest.join("dump.raw")).unwrap().len(), 1024);

    // teardown : buckets supprimés
    ops::teardown_case(&cfg, id).await.expect("teardown_case");
    let admin = intake_core::s3_dataplane::S3DataPlane::new(build_client(
        &cfg,
        &cfg.admin_access_key,
        &cfg.admin_secret_key,
        None,
    ));
    assert!(admin
        .client
        .head_bucket()
        .bucket(&created.case.data_bucket)
        .send()
        .await
        .is_err());
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-core --test end_to_end`
Expected: FAIL à la compilation (fonctions `ops` inexistantes).

- [ ] **Step 3 : Implémenter `crates/core/src/ops.rs`**

```rust
use crate::case::{Case, CaseState};
use crate::config::Config;
use crate::minio_iam::MinioIam;
use crate::s3_dataplane::{build_client, S3DataPlane};
use std::path::{Path, PathBuf};

pub struct CreateResult {
    pub case: Case,
    pub scoped_secret_key: String,
}

fn rand_suffix() -> String {
    let mut b = [0u8; 4];
    getrandom::getrandom(&mut b).expect("rng");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn slug(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

fn admin_dp(cfg: &Config) -> S3DataPlane {
    S3DataPlane::new(build_client(cfg, &cfg.admin_access_key, &cfg.admin_secret_key, None))
}

pub async fn create_case(cfg: &Config, id: &str) -> anyhow::Result<CreateResult> {
    let s = slug(id);
    let suffix = rand_suffix();
    let data_bucket = format!("intake-data-{s}-{suffix}");
    let site_bucket = format!("intake-site-{s}-{suffix}");
    let dp = admin_dp(cfg);

    // bucket data : privé + versioning + CORS (belt-and-suspenders pour la prod)
    dp.create_bucket(&data_bucket).await?;
    dp.enable_versioning(&data_bucket).await?;
    dp.set_cors(&data_bucket, "*").await?;

    // bucket site : public read + déploiement
    dp.create_bucket(&site_bucket).await?;
    dp.put_public_read_policy(&site_bucket).await?;

    // config.json non-secret injecté pour le site
    let config_json = serde_json::json!({
        "endpoint": cfg.endpoint,
        "region": cfg.region,
        "dataBucket": data_bucket,
        "usePathStyle": true
    })
    .to_string();
    dp.put_object_bytes(
        &site_bucket,
        "site/config.json",
        config_json.into_bytes(),
        "application/json",
    )
    .await?;

    // déployer les fichiers statiques du site (index.html, sigv4.js, upload.js…)
    let site_dir = Path::new(&cfg.site_dir);
    if site_dir.exists() {
        dp.deploy_dir(&site_bucket, site_dir, "site/").await?;
    }

    // clé scopée écriture-seule pour la victime
    let iam = MinioIam::from_config(cfg);
    let creds = iam.create_scoped_upload_key(&data_bucket).await?;

    let site_url = format!("{}/{}/site/index.html", cfg.endpoint, site_bucket);
    let case = Case {
        id: id.to_string(),
        data_bucket,
        site_bucket,
        scoped_access_key: creds.access_key,
        site_url,
        state: CaseState::Active,
    };
    case.save(&cfg.cases_dir)?;

    Ok(CreateResult {
        case,
        scoped_secret_key: creds.secret_key,
    })
}

pub async fn pull_case(
    cfg: &Config,
    id: &str,
    dest: &Path,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let case = Case::load(&cfg.cases_dir, id)?;
    let dp = admin_dp(cfg);
    dp.download_all(&case.data_bucket, dest).await
}

pub async fn teardown_case(cfg: &Config, id: &str) -> anyhow::Result<()> {
    let case = match Case::load(&cfg.cases_dir, id) {
        Ok(c) => c,
        Err(_) => return Ok(()), // rien à faire
    };
    let iam = MinioIam::from_config(cfg);
    // révoquer la clé (ignore l'erreur si déjà supprimée)
    let _ = iam.delete_scoped_key(&case.scoped_access_key).await;
    let dp = admin_dp(cfg);
    let _ = dp.delete_bucket(&case.data_bucket).await;
    let _ = dp.delete_bucket(&case.site_bucket).await;

    let torn = Case {
        state: CaseState::TornDown,
        ..case
    };
    torn.save(&cfg.cases_dir)?;
    Ok(())
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-core --test end_to_end`
Expected: PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/ops.rs crates/core/tests/end_to_end.rs Cargo.lock
git commit -m "feat(core): create/pull/teardown case orchestration + e2e test"
```

---

### Task 7 : Façade CLI `intake` + README

**Files:**
- Modify: `crates/cli/src/main.rs`
- Create: `README.md`
- Test: `crates/cli/tests/cli_smoke.rs`

**Interfaces:**
- Consumes: `intake_core::{config::Config, ops}`.
- Produces (binaire) : `intake create-case <id>`, `intake pull-case <id> [--dest DIR]`, `intake teardown-case <id> [--yes]`.

- [ ] **Step 1 : Écrire le test `crates/cli/tests/cli_smoke.rs`**

```rust
// Vérifie que le binaire s'exécute et pilote un cycle complet.
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_intake"))
}

#[test]
fn cli_create_and_teardown() {
    let id = "cli-smoke-0001";
    // teardown préventif (ignore le code retour)
    let _ = bin().args(["teardown-case", id, "--yes"]).status();

    let create = bin().args(["create-case", id]).output().unwrap();
    assert!(create.status.success(), "create-case a échoué : {create:?}");
    let stdout = String::from_utf8_lossy(&create.stdout);
    assert!(stdout.contains("site_url"), "sortie inattendue : {stdout}");

    let teardown = bin().args(["teardown-case", id, "--yes"]).output().unwrap();
    assert!(teardown.status.success(), "teardown-case a échoué : {teardown:?}");
}
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `cargo test -p intake-cli --test cli_smoke`
Expected: FAIL (le binaire n'a pas encore les sous-commandes ; `create-case` renvoie une erreur / sortie inattendue).

- [ ] **Step 3 : Implémenter `crates/cli/src/main.rs`**

```rust
use clap::{Parser, Subcommand};
use intake_core::config::Config;
use intake_core::ops;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "intake", about = "Service de gestion d'intake d'incident (S3)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Crée une affaire : buckets, versioning, site, clé scopée.
    CreateCase { id: String },
    /// Récupère les données d'une affaire.
    PullCase {
        id: String,
        #[arg(long, default_value = "pulled")]
        dest: String,
    },
    /// Détruit une affaire : clé scopée + buckets.
    TeardownCase {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::from_env();

    match cli.cmd {
        Cmd::CreateCase { id } => {
            let r = ops::create_case(&cfg, &id).await?;
            // Sortie : URL du site + les 3 valeurs à remettre à la victime (hors-bande).
            println!("case_id: {}", r.case.id);
            println!("site_url: {}", r.case.site_url);
            println!("data_bucket: {}", r.case.data_bucket);
            println!("--- credentials à remettre à la victime (hors-bande) ---");
            println!("access_key: {}", r.case.scoped_access_key);
            println!("secret_key: {}", r.scoped_secret_key);
            println!("session_token: (aucun sur MinIO)");
        }
        Cmd::PullCase { id, dest } => {
            let files = ops::pull_case(&cfg, &id, &PathBuf::from(&dest)).await?;
            println!("{} objet(s) récupéré(s) dans {dest} :", files.len());
            for (k, _) in files {
                println!("  {k}");
            }
        }
        Cmd::TeardownCase { id, yes } => {
            if !yes {
                anyhow::bail!("Refus : ajouter --yes pour confirmer la destruction de l'affaire '{id}'.");
            }
            ops::teardown_case(&cfg, &id).await?;
            println!("Affaire '{id}' détruite (clé + buckets).");
        }
    }
    Ok(())
}
```

- [ ] **Step 4 : Lancer le test et vérifier qu'il passe**

Run: `cargo test -p intake-cli --test cli_smoke`
Expected: PASS.

- [ ] **Step 5 : Écrire `README.md` (démarrage)**

````markdown
# intake — service de gestion (prototype offline)

## Prérequis
- Docker (pour MinIO), Rust (stable), `mc` (MinIO Client) sur l'hôte.

## Démarrage
```bash
docker compose up -d
source config.example.env          # ou vos propres variables INTAKE_*
bash scripts/bootstrap-minio.sh    # pose l'alias mc + attend MinIO
cargo build --release
```

## Cycle d'une affaire
```bash
./target/release/intake create-case acme-2026     # crée buckets/site/clé, imprime l'URL + creds
./target/release/intake pull-case  acme-2026 --dest ./pulled
./target/release/intake teardown-case acme-2026 --yes
```

## Tests
```bash
cargo test                          # nécessite MinIO lancé + bootstrap fait
```
````

- [ ] **Step 6 : Commit**

```bash
git add crates/cli/src/main.rs crates/cli/tests/cli_smoke.rs README.md Cargo.lock
git commit -m "feat(cli): intake create-case/pull-case/teardown-case + README"
```

---

## Auto-revue (couverture de la spec)

- §2/§8 modèle Option A + clé scopée écriture-seule → Task 4 (policy + tests négatifs).
- §5.1 MinIO env → Task 1.
- §5.2 workspace core + cli, adaptateur (S3DataPlane / MinioIam) → Tasks 1–4, 6.
- §5.4 récupération + (hash : voir note) → Task 6 (`pull_case`).
- §6 modèle `Case` → Task 5.
- §7 flux create/pull/teardown → Task 6, exposés en CLI Task 7.
- §13 deux buckets, persistance JSON → Tasks 5–6.

**Écart connu (assumé pour ce plan) :** la **vérification SHA-256 vs manifeste hors-bande**
(§5.4/§13) n'est pas implémentée dans Plan A. `pull_case` télécharge et liste ; la
comparaison de hash sera ajoutée soit en fin de Plan A (petite tâche `verify_hashes`),
soit avec Plan B (le site produit les hashes). À trancher avec l'utilisateur.
```
