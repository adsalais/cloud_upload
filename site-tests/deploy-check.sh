#!/usr/bin/env bash
# Automated site-deployment check: create-case, then public GET of the served files,
# then teardown. Complements the manual browser test (MANUAL-E2E.md).
# bin/ must be on PATH (for mc). Binary: $INTAKE_BIN (default: release).
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
grep -q '"dataBucket"' "/tmp/dc-config.json" || { echo "config.json missing dataBucket"; fail=1; }
grep -q 'multipartUpload' "/tmp/dc-upload.js" || { echo "upload.js incomplete"; fail=1; }

# Security: the data/ prefix must NOT be publicly readable.
DATA_URL="${BASE%site/}data/should-not-be-public"
dcode="$(curl -s -o /dev/null -w '%{http_code}' "$DATA_URL")"
if [ "$dcode" = "200" ]; then echo "FAIL data/ is public ($dcode)"; fail=1; else echo "OK   data/ private ($dcode)"; fi

"$BIN" teardown-case "$ID" --yes >/dev/null
rm -f /tmp/dc-*
if [ "$fail" = 0 ]; then echo "DEPLOY-CHECK: PASS"; else echo "DEPLOY-CHECK: FAIL"; exit 1; fi
