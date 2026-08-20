# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Discreet DFIR incident-data intake on S3-compatible object storage: a victim uploads
files to a per-case bucket via a static site; the response team pulls them with the S3
API and destroys the bucket. Prototyped offline with MinIO; the S3 data plane is portable
to French providers (Scaleway/OVH) by changing the endpoint. Design and implementation
plans live in `docs/superpowers/`.

## Prerequisites & environment

- **Docker** runs MinIO **and** the `mc` client — there is no host install of `mc`.
- **Rust** (stable); **Node ≥ 20** (test runner only — the site ships zero dependencies).
- `bin/mc` is a wrapper that runs the official `minio/mc` image via Docker
  (`--network host`, stateless auth via `MC_HOST_<alias>`). **Anything that shells to
  `mc` — the Rust IAM control plane and the shell scripts — needs `bin/` on `PATH`.**
  `source config.example.env` sets that up (plus the `INTAKE_*` config vars).

## Common commands

```bash
docker compose up -d                 # start MinIO (console on :9001)
source config.example.env            # INTAKE_* vars + bin/ on PATH (needed for mc)
bash scripts/bootstrap-minio.sh      # wait for MinIO + validate the mc wrapper
cargo build --release

# Rust tests (need MinIO running + bin/ on PATH):
cargo test
cargo test -p intake-core --test scoped_key            # a single integration test file
cargo test -p intake-core --test scoped_key -- --nocapture

# Site tests (zero-dep, Node built-in runner; need MinIO + a provisioned scoped key):
eval "$(bash site-tests/provision.sh)"                 # mints a throwaway bucket + scoped key
node --test site-tests/*.test.mjs                      # NOT `site-tests/` — Node 23 rejects a dir arg
bash site-tests/deprovision.sh

# Automated site-deployment check (create-case → GET the served files → teardown):
bash site-tests/deploy-check.sh                        # uses target/release/intake; INTAKE_BIN overrides

# CLI lifecycle:
./target/release/intake create-case <id>               # buckets + site + scoped key; prints URL + creds
./target/release/intake pull-case <id> --dest ./pulled
./target/release/intake teardown-case <id> --yes
```

## Architecture

A Cargo workspace plus a dependency-free static site.

- `crates/core` (`intake-core`) — reusable logic, split along the one boundary that
  matters for portability:
  - `s3_dataplane.rs` — **portable** S3 operations via `aws-sdk-s3` (path-style): bucket
    create / versioning / CORS / delete-with-version-purge, public-read policy, object
    put / `deploy_dir` / `download_all`. Runs unchanged on MinIO/Scaleway/OVH.
  - `minio_iam.rs` — **provider-specific** control plane: mints/revokes the scoped
    write-only service-account key by shelling to `mc`. This is the ONLY piece that must
    be reimplemented per provider (a future `scaleway_iam` / `ovh_iam`).
  - `ops.rs` — orchestration (`create_case` / `pull_case` / `teardown_case`) composing
    the two planes + `case.rs` (JSON-persisted `Case` state under `cases/`).
- `crates/cli` (`intake`) — thin `clap` facade over `ops`. A future internal web UI would
  be a second binary over the same `core`.
- `site/` — the victim-facing upload page, **zero dependencies, no build step**:
  `sigv4.js` (AWS SigV4 via Web Crypto), `upload.js` (resumable multipart over `fetch`),
  `index.html`. `create_case` deploys these under `site/` in the per-case bucket, plus a
  per-case `site/config.json` (endpoint / region / dataBucket). Uploads land under
  `data/`.

### Security model (why it's shaped this way)

**One bucket per case with prefix isolation** (constants `SITE_PREFIX`/`DATA_PREFIX` in
`ops.rs`): public read is granted on `site/*` **only**; the uploaded evidence under
`data/*` stays private. The victim's scoped key may only `PutObject` + multipart under
`data/*` — never Get/List, never `site/*` (so a leaked key can't overwrite the served JS
to attack the next victim). Versioning is on. Scaleway/OVH offer no STS, so the scoped
key is long-lived and destroyed at teardown; its blast radius is one throwaway bucket.
`crates/core/tests/scoped_key.rs` asserts these negatives and
`site_and_objects.rs` asserts `site/` public + `data/` private (403) — keep them green.

### Gotchas

- **Same-origin path-style** means CORS isn't needed in the prototype; `set_cors`
  deliberately swallows MinIO's `NotImplemented` (CORS matters only for prod virtual-host).
- `cases/` holds scoped credentials and is git-ignored (root and nested). Gitignore does
  not accept end-of-line comments — keep patterns on their own line.
- The SHA-256 integrity manifest is intentionally out of scope for the prototype.

## Commit convention

Do **not** add `Co-Authored-By` or `Claude-Session` trailers to commit messages.
