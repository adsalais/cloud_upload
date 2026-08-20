#!/usr/bin/env bash
# Attend que MinIO soit prêt et vérifie que le wrapper `mc` (Docker) fonctionne.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$ROOT/bin:$PATH"

ALIAS="${INTAKE_MC_ALIAS:-myminio}"
ENDPOINT="${INTAKE_ENDPOINT:-http://localhost:9000}"

echo "Attente de MinIO sur $ENDPOINT ..."
for _ in $(seq 1 60); do
  if curl -fsS "$ENDPOINT/minio/health/live" >/dev/null 2>&1; then
    echo "MinIO en ligne."
    break
  fi
  sleep 1
done

# Confirme le wrapper mc (Docker) et pré-tire l'image minio/mc.
mc admin info "$ALIAS" >/dev/null
echo "mc (Docker) opérationnel — alias '$ALIAS' via MC_HOST."
