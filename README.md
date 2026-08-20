# intake — discreet incident-data intake (prototype)

A DFIR tool to discreetly collect data from an affected party during incident response,
**with no obvious link between the response team and the upload endpoint**.

For each case, the tool creates an **S3-compatible bucket** hosting a small static upload
page. The client drops their files from a browser (or from the command line — see below);
the team retrieves them via the S3 API, then the bucket is destroyed. Prototyped offline
with **MinIO** (Docker); the S3 code is portable to any S3-compatible object storage
provider by changing the endpoint.

## How it works

- **One bucket per case** (`intake-<id>-<rand>`) with **prefix isolation**:
  - `site/*` → **public read** (serves `index.html`, `sigv4.js`, `upload.js`, `config.json`).
  - `data/*` → **private** (the uploaded files).
- The upload page is **dependency-free** (vanilla JS: AWS SigV4 via Web Crypto +
  resumable multipart). Nothing to build, no `node_modules`.

### The two roles / credentials (important)

| Role | Credential | Used for | Scope |
|------|------------|----------|-------|
| **Your team** | **admin** creds (`INTAKE_ADMIN_*`, default `minioadmin`) | `create-case`, `pull-case`, `teardown-case` | administrator |
| **The client** | per-case **scoped key**, printed by `create-case` | uploading (web page or CLI) | **write-only under `data/*`** |

The client's key **cannot** read, list, or write anywhere except under `data/` (so it
can't see other data or tamper with the site). Retrieval uses **your admin credentials** —
a **different** identity from the client's.

## Prerequisites
- **Docker** (runs MinIO **and** the `mc` client — no host install of `mc`).
- **Rust** (stable).
- **Node ≥ 20** (only for the site tests).

## Setup
```bash
docker compose up -d                 # start MinIO (console at http://localhost:9001)
source config.example.env            # INTAKE_* vars + put bin/ on PATH (needed for mc)
bash scripts/bootstrap-minio.sh      # wait for MinIO + validate the mc wrapper
cargo build --release
```
> `bin/mc` wraps the official `minio/mc` image via Docker. `source config.example.env`
> puts `bin/` on `PATH`; any command using `mc` needs it.

## End-to-end walkthrough

### 1. Create a case (team → admin creds)
```bash
./target/release/intake create-case demo
```
Output (example):
```
case_id: demo
site_url: http://localhost:9000/intake-demo-2c2e6987/site/index.html
endpoint: http://localhost:9000
bucket: intake-demo-2c2e6987
--- credentials to hand to the client (out of band) ---
access_key: 3BKWLK7G91BC6UPY1532
secret_key: y+Qs18nB5TU1ynWe4XjWuwcJ9G9NXJ8ozNDm+iQW
session_token: (none on MinIO)
```
Hand the `site_url` (or the `endpoint` + `bucket`) and the two credential values to the
client over a **separate channel** (out of band).

### 2a. Upload from a browser (non-technical client)
- **Which URL?** the `site_url` above. Open it in a browser.
  *(On `localhost` the context is "secure" so Web Crypto works; in production it's HTTPS.)*
- **Which IAM?** the **scoped key** from step 1: paste `access_key` into "Access key",
  `secret_key` into "Secret key", leave "Session token" **empty** (MinIO has none).
- Drop a file into the drop zone. The progress bar advances, then "Done ✔". The file is
  written under `data/…` in the case bucket.

Tip: pre-fill the credentials via the URL fragment —
`…/site/index.html#ak=<access_key>&sk=<secret_key>`.

### 2b. Upload from the command line (technical client / very large files)

For a technical client, or for multi-GB forensic images, any S3-compatible CLI works with
the **same scoped key** — no browser needed. The key is **write-only under `data/`**: it
can `PutObject` (and multipart) there, but cannot list or read, so point the tool at the
`data/` prefix and skip any bucket-listing step.

You need three things from the `create-case` output: `endpoint`, `bucket`, and the scoped
`access_key` / `secret_key`.

**MinIO client (`mc`)** — verified against this prototype; auto-multiparts large files:
```bash
mc alias set case <endpoint> <access_key> <secret_key>
mc cp ./dump.raw case/<bucket>/data/
mc cp --recursive ./evidence-dir/ case/<bucket>/data/
```

**AWS CLI** — multiparts large files automatically:
```bash
export AWS_ACCESS_KEY_ID=<access_key>
export AWS_SECRET_ACCESS_KEY=<secret_key>
export AWS_DEFAULT_REGION=<region>                  # avoids a bucket-location lookup the key can't do
aws configure set default.s3.addressing_style path  # path-style (MinIO and many providers)
aws --endpoint-url <endpoint> s3 cp ./dump.raw s3://<bucket>/data/
```

**rclone** — resumable, good for very large uploads:
```bash
rclone copy ./dump.raw \
  ":s3,provider=Other,access_key_id=<access_key>,secret_access_key=<secret_key>,endpoint=<endpoint>:<bucket>/data/" \
  --s3-no-check-bucket --s3-force-path-style
```
`--s3-no-check-bucket` is required because the write-only key cannot check/list the bucket.

> Integrity: a technical client can compute hashes with `sha256sum` and send them out of
> band, or build the manifest with `intake manifest` (see step 3).

### 3. Retrieve (team → admin creds, a different IAM)
```bash
./target/release/intake pull-case demo --dest ./pulled
```
This uses **your admin credentials** (not the client's key) and downloads only the `data/`
prefix. Files land under `./pulled/data/…`.

**Integrity verification (chain of custody).** Before uploading, whoever holds the files
(the client, or you on a reference copy) computes a SHA-256 manifest and sends it to you
**out of band**:
```bash
./target/release/intake manifest dump.raw memory.img --out data-manifest.json
# -> { "dump.raw": "9f2b...", "memory.img": "3ad0..." }
```
At retrieval, pass it to `pull-case`: each object is re-hashed and compared **by content**
(robust to the `data/<timestamp>-` key prefix):
```bash
./target/release/intake pull-case demo --dest ./pulled --manifest data-manifest.json
# OK / MISSING / UNEXPECTED per object; exits non-zero on any divergence.
```

### 4. Destroy the case
```bash
./target/release/intake teardown-case demo --yes
```
Revokes the scoped key **and** deletes the bucket (site + data).

## Automated tests
```bash
source config.example.env

# Rust (service) — needs MinIO running + bin/ on PATH:
cargo test

# Site (dependency-free, Node built-in runner):
eval "$(bash site-tests/provision.sh)"
node --test site-tests/*.test.mjs     # NOT `site-tests/` — Node 23 rejects a directory arg
bash site-tests/deprovision.sh

# Site deployment (create-case -> public GET of served files -> data/ private -> teardown):
bash site-tests/deploy-check.sh
```
What this proves: correct SigV4 signature and real multipart accepted by the storage; the
scoped key can write under `data/` but **cannot** read/list/tamper with the site; `site/`
is public while `data/` returns 403 anonymously.

## Stop
```bash
docker compose down          # stop MinIO (add -v to also wipe ./.minio-data)
```

## Layout
- `crates/core` — library: **portable** S3 data plane (`aws-sdk-s3`, path-style) +
  **provider-specific** IAM control plane (MinIO via `mc`) + integrity (`sha2`).
- `crates/cli` — the `intake` binary (CLI facade; a future internal admin web UI would
  reuse the same core).
- `site/` — dependency-free upload page (SigV4 Web Crypto + multipart).
- `bin/mc` — Docker wrapper for the MinIO client.
- `docs/superpowers/` — design and implementation plans.

## Toward production
The only provider-specific piece is the **IAM control plane** (minting/revoking the scoped
key): add an adapter for your S3-compatible provider and change the endpoint. Everything
else (S3 data plane, static site, multipart) is already portable. The scoped key is
long-lived and destroyed at teardown; if your provider offers STS, you could issue
short-lived credentials instead. See `docs/superpowers/`.
