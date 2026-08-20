import { test } from 'node:test';
import assert from 'node:assert';
import { execFileSync } from 'node:child_process';
import { multipartUpload, putPartWithRetry } from '../site/upload.js';

// --- Unitaire : la reprise (retry) d'une part qui échoue une fois ---
test('putPartWithRetry réessaie puis réussit', async () => {
  let calls = 0;
  const orig = globalThis.fetch;
  globalThis.fetch = async () => {
    calls++;
    if (calls < 2) return { ok: false, status: 500, text: async () => 'boom' };
    return { ok: true, status: 200, headers: { get: () => '"etag123"' }, text: async () => '' };
  };
  try {
    const etag = await putPartWithRetry(
      {
        base: { endpoint: 'http://localhost:9000', region: 'us-east-1', accessKey: 'a', secretKey: 'b' },
        uri: '/bucket/key',
        uploadId: 'uid',
        partNumber: 1,
        chunk: new Uint8Array([1, 2, 3]),
      },
      3
    );
    assert.strictEqual(etag, '"etag123"');
    assert.strictEqual(calls, 2);
  } finally {
    globalThis.fetch = orig;
  }
});

// --- Intégration : multipart réel (3 parts) contre MinIO ---
const BUCKET = process.env.TEST_DATA_BUCKET;
const ENDPOINT = process.env.INTAKE_ENDPOINT || 'http://localhost:9000';
const REGION = process.env.INTAKE_REGION || 'us-east-1';
const ALIAS = process.env.INTAKE_MC_ALIAS || 'myminio';

test('multipart réel (3 parts) accepté par MinIO', { skip: !BUCKET }, async () => {
  const key = 'multipart/big.bin';
  const size = 20 * 1024 * 1024; // 20 MiB → 3 parts @ 8 MiB
  const body = new Uint8Array(size);
  for (let i = 0; i < size; i++) body[i] = i & 0xff;

  let last = 0;
  const r = await multipartUpload({
    endpoint: ENDPOINT,
    region: REGION,
    bucket: BUCKET,
    key,
    body,
    creds: { accessKey: process.env.TEST_SCOPED_AK, secretKey: process.env.TEST_SCOPED_SK },
    onProgress: (p) => {
      last = p.uploaded;
    },
  });
  assert.ok(r.parts.length >= 3, `attendu >=3 parts, obtenu ${r.parts.length}`);
  assert.strictEqual(last, size);

  // Vérifier la taille côté serveur via mc (admin).
  const out = execFileSync('mc', ['--json', 'stat', `${ALIAS}/${BUCKET}/${key}`]).toString();
  const stat = JSON.parse(out.trim().split('\n')[0]);
  assert.strictEqual(Number(stat.size), size);
});
