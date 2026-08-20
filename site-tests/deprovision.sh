#!/usr/bin/env bash
# Clean up the test bucket + key (bin/ must be on PATH for mc).
set -euo pipefail
ALIAS="${INTAKE_MC_ALIAS:-myminio}"
[ -n "${TEST_SCOPED_AK:-}" ] && mc admin user svcacct rm "$ALIAS" "$TEST_SCOPED_AK" || true
[ -n "${TEST_DATA_BUCKET:-}" ] && mc rb --force "$ALIAS/$TEST_DATA_BUCKET" || true
