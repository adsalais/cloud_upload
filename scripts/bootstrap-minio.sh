#!/usr/bin/env bash
# Wait for MinIO to be ready and check that the `mc` wrapper (Docker) works.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$ROOT/bin:$PATH"

ALIAS="${INTAKE_MC_ALIAS:-myminio}"
ENDPOINT="${INTAKE_ENDPOINT:-http://localhost:9000}"

echo "Waiting for MinIO at $ENDPOINT ..."
for _ in $(seq 1 60); do
  if curl -fsS "$ENDPOINT/minio/health/live" >/dev/null 2>&1; then
    echo "MinIO is up."
    break
  fi
  sleep 1
done

# Confirm the mc (Docker) wrapper works and pre-pull the image.
mc admin info "$ALIAS" >/dev/null
echo "mc (Docker) OK - alias '$ALIAS' via MC_HOST."
