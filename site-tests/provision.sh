#!/usr/bin/env bash
# Provisions a throwaway bucket + a write-only scoped key and prints exports.
# Usage: eval "$(bash site-tests/provision.sh)"   (bin/ must be on PATH for mc)
set -euo pipefail
ALIAS="${INTAKE_MC_ALIAS:-myminio}"
PARENT="${INTAKE_MC_PARENT:-minioadmin}"
BUCKET="site-test-$$"
mc mb "$ALIAS/$BUCKET" >/dev/null
POL="$(mktemp)"
cat > "$POL" <<EOF
{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:PutObject","s3:AbortMultipartUpload","s3:ListMultipartUploadParts"],"Resource":["arn:aws:s3:::$BUCKET/*"]}]}
EOF
CREDS="$(mc --json admin user svcacct add --policy "$POL" "$ALIAS" "$PARENT")"
rm -f "$POL"
AK="$(printf '%s' "$CREDS" | grep -o '"accessKey":"[^"]*"' | cut -d'"' -f4)"
SK="$(printf '%s' "$CREDS" | grep -o '"secretKey":"[^"]*"' | cut -d'"' -f4)"
echo "export TEST_DATA_BUCKET=$BUCKET"
echo "export TEST_SCOPED_AK=$AK"
echo "export TEST_SCOPED_SK=$SK"
