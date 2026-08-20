#!/usr/bin/env bash
# Vérifie automatiquement le déploiement du site : create-case, puis GET public des
# fichiers servis, puis teardown. Complète le test navigateur manuel (MANUAL-E2E.md).
# bin/ doit être sur le PATH (pour mc). Binaire : $INTAKE_BIN (défaut : release).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$ROOT/bin:$PATH"
BIN="${INTAKE_BIN:-$ROOT/target/release/intake}"
ID="deploy-check-$$"

out="$("$BIN" create-case "$ID")"
printf '%s\n' "$out"
SITE_URL="$(printf '%s\n' "$out" | awk -F': ' '/^site_url:/{print $2}')"
BASE="${SITE_URL%index.html}"   # .../site/

fail=0
for f in index.html sigv4.js upload.js config.json; do
  code="$(curl -s -o "/tmp/dc-$f" -w '%{http_code}' "${BASE}${f}")"
  if [ "$code" = "200" ]; then echo "OK   $f ($code)"; else echo "FAIL $f ($code)"; fail=1; fi
done
grep -q '"dataBucket"' "/tmp/dc-config.json" || { echo "config.json sans dataBucket"; fail=1; }
grep -q 'multipartUpload' "/tmp/dc-upload.js" || { echo "upload.js incomplet"; fail=1; }

# Sécurité : le préfixe data/ ne doit PAS être lisible publiquement.
DATA_URL="${BASE%site/}data/should-not-be-public"
dcode="$(curl -s -o /dev/null -w '%{http_code}' "$DATA_URL")"
if [ "$dcode" = "200" ]; then echo "FAIL data/ est public ($dcode)"; fail=1; else echo "OK   data/ privé ($dcode)"; fi

"$BIN" teardown-case "$ID" --yes >/dev/null
rm -f /tmp/dc-*
if [ "$fail" = 0 ]; then echo "DEPLOY-CHECK: PASS"; else echo "DEPLOY-CHECK: FAIL"; exit 1; fi
