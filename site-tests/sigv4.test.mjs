import { test } from 'node:test';
import assert from 'node:assert';
import {
  buildCanonicalRequest,
  encodePath,
  canonicalQueryString,
  signRequest,
} from '../site/sigv4.js';

test('encodePath encodes each segment, preserves slashes', () => {
  assert.strictEqual(encodePath('bucket/a b/c.txt'), '/bucket/a%20b/c.txt');
});

test('canonicalQueryString sorts and encodes', () => {
  assert.strictEqual(
    canonicalQueryString({ uploadId: 'a b', partNumber: '2' }),
    'partNumber=2&uploadId=a%20b'
  );
});

test('buildCanonicalRequest assembles the exact SigV4 string', () => {
  const { canonicalRequest, signedHeaders } = buildCanonicalRequest({
    method: 'PUT',
    canonicalUri: '/mybucket/evidence/a.txt',
    canonicalQuery: 'partNumber=1&uploadId=abc',
    headers: {
      host: 'localhost:9000',
      'x-amz-date': '20260820T120000Z',
      'x-amz-content-sha256': 'UNSIGNED-PAYLOAD',
    },
    payloadHash: 'UNSIGNED-PAYLOAD',
  });
  assert.strictEqual(signedHeaders, 'host;x-amz-content-sha256;x-amz-date');
  const expected =
    'PUT\n' +
    '/mybucket/evidence/a.txt\n' +
    'partNumber=1&uploadId=abc\n' +
    'host:localhost:9000\n' +
    'x-amz-content-sha256:UNSIGNED-PAYLOAD\n' +
    'x-amz-date:20260820T120000Z\n' +
    '\n' +
    'host;x-amz-content-sha256;x-amz-date\n' +
    'UNSIGNED-PAYLOAD';
  assert.strictEqual(canonicalRequest, expected);
});

// --- Integration: real signed PUT accepted by MinIO (skipped if not provisioned) ---
const BUCKET = process.env.TEST_DATA_BUCKET;
const ENDPOINT = process.env.INTAKE_ENDPOINT || 'http://localhost:9000';
const REGION = process.env.INTAKE_REGION || 'us-east-1';

test('real signed PUT accepted by MinIO', { skip: !BUCKET }, async () => {
  const key = 'sigtest/hello.txt';
  const { url, headers } = await signRequest({
    method: 'PUT',
    endpoint: ENDPOINT,
    canonicalUri: encodePath(`${BUCKET}/${key}`),
    region: REGION,
    accessKey: process.env.TEST_SCOPED_AK,
    secretKey: process.env.TEST_SCOPED_SK,
  });
  const res = await fetch(url, { method: 'PUT', headers, body: new TextEncoder().encode('hello') });
  assert.strictEqual(res.status, 200, `PUT status ${res.status} - invalid signature? ${await res.text()}`);
});
