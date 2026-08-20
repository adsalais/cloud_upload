# Plan B — Site d'upload zéro-dépendance — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un site statique **sans aucune dépendance tierce** qui permet à une victime de déposer des fichiers (y compris multi-Go) sur le bucket de données S3 d'une affaire, via un upload **multipart reprenable** signé en **AWS SigV4** avec la **Web Crypto API**.

**Architecture:** Trois fichiers JS natifs — `sigv4.js` (canonicalisation + signature SigV4 via `crypto.subtle`), `upload.js` (orchestration multipart sur `fetch`), `index.html` (UI : credentials + glisser-déposer + barre de progression). La config non-secrète (endpoint, région, bucket) arrive via un `config.json` déposé par affaire (Plan A). Les mêmes `.js` s'exécutent au navigateur et sous le runner de test intégré de Node.

**Tech Stack:** JavaScript ESM natif, **Web Crypto API**, `fetch`. Aucune bibliothèque, aucun build, aucun `node_modules`. Node ≥ 20 uniquement comme **lanceur de tests** (`node --test`). MinIO + `mc` pour provisionner les credentials de test d'intégration.

## Global Constraints

- **Zéro dépendance tierce** dans le site *et* dans les tests : uniquement les API natives (`crypto.subtle`, `fetch`, `TextEncoder`, `URL`) — communes au navigateur et à Node ≥ 20. **Jamais** de `npm install`, jamais de `node_modules`.
- Fichiers du site en **ESM `.js`** ; `site/package.json` réduit à `{"type":"module","private":true}` (config de module, **pas** une dépendance).
- **SigV4** pour S3, **path-style**, `x-amz-content-sha256: UNSIGNED-PAYLOAD` sur toutes les requêtes (évite de re-hasher les chunks).
- L'en-tête `host` est **inclus dans la signature** mais **jamais posé** sur `fetch` (interdit côté navigateur) — le navigateur/Node l'ajoute avec la même valeur.
- `crypto.subtle` exige un contexte sécurisé : OK sur `http://localhost` (proto) et HTTPS (prod).
- **Dépend de Plan A** : `create_case` déploie `./site/` dans le bucket site et écrit `site/config.json`. Les tests d'intégration provisionnent un bucket + une clé scopée via `site-tests/provision.sh` (MinIO + `mc`).
- Node ≥ 20 requis pour les tests (`globalThis.crypto.subtle` + `fetch` natifs).

---

### Task 1 : `sigv4.js` — signature SigV4 (Web Crypto) + tests

**Files:**
- Create: `site/package.json`
- Create: `site/sigv4.js`
- Create: `site-tests/provision.sh`
- Create: `site-tests/deprovision.sh`
- Test: `site-tests/sigv4.test.mjs`

**Interfaces:**
- Produces (exports de `site/sigv4.js`) :
  - `sha256Hex(data: string|Uint8Array) => Promise<string>`
  - `signingKey(secret, dateStamp, region, service) => Promise<Uint8Array>`
  - `encodeRfc3986(str) => string`
  - `encodePath(key) => string` (ex. `"bucket/a/b.txt"` → `"/bucket/a/b.txt"`, segments encodés)
  - `canonicalQueryString(query: object) => string`
  - `buildCanonicalRequest({method, canonicalUri, canonicalQuery, headers, payloadHash}) => {canonicalRequest, signedHeaders}` (**pur**, synchrone)
  - `signRequest(opts) => Promise<{url, headers}>` où `opts = {method, endpoint, canonicalUri, query?, payloadHash?, region, service?, accessKey, secretKey, sessionToken?, amzDate?, extraHeaders?}`

- [ ] **Step 1 : Écrire `site/package.json` (config de module, zéro dépendance)**

```json
{
  "type": "module",
  "private": true
}
```

- [ ] **Step 2 : Écrire le test unitaire exact `site-tests/sigv4.test.mjs` (canonical request — hors-ligne, sans MinIO)**

```javascript
import { test } from 'node:test';
import assert from 'node:assert';
import { buildCanonicalRequest, encodePath, canonicalQueryString } from '../site/sigv4.js';

test('encodePath encode chaque segment, préserve les slashes', () => {
  assert.strictEqual(encodePath('bucket/a b/c.txt'), '/bucket/a%20b/c.txt');
});

test('canonicalQueryString trie et encode', () => {
  assert.strictEqual(
    canonicalQueryString({ uploadId: 'a b', partNumber: '2' }),
    'partNumber=2&uploadId=a%20b'
  );
});

test('buildCanonicalRequest assemble exactement la chaîne SigV4', () => {
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
```

- [ ] **Step 3 : Lancer le test pour vérifier qu'il échoue**

Run: `node --test site-tests/sigv4.test.mjs`
Expected: FAIL (module `../site/sigv4.js` introuvable / exports manquants).

- [ ] **Step 4 : Implémenter `site/sigv4.js`**

```javascript
// sigv4.js — signature AWS SigV4, ZÉRO dépendance (Web Crypto).
// Primitives crypto = crypto.subtle (navigateur/Node). On n'écrit que la
// canonicalisation (formatage de chaînes), testée exactement + contre MinIO.

const encoder = new TextEncoder();

function toHex(bytes) {
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
}

export async function sha256Hex(data) {
  const buf = typeof data === 'string' ? encoder.encode(data) : data;
  const digest = await crypto.subtle.digest('SHA-256', buf);
  return toHex(new Uint8Array(digest));
}

async function hmac(keyBytes, msg) {
  const key = await crypto.subtle.importKey(
    'raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']
  );
  const sig = await crypto.subtle.sign('HMAC', key, encoder.encode(msg));
  return new Uint8Array(sig);
}

export async function signingKey(secret, dateStamp, region, service) {
  const kDate = await hmac(encoder.encode('AWS4' + secret), dateStamp);
  const kRegion = await hmac(kDate, region);
  const kService = await hmac(kRegion, service);
  return hmac(kService, 'aws4_request');
}

export function encodeRfc3986(str) {
  return encodeURIComponent(str).replace(
    /[!'()*]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase()
  );
}

export function encodePath(key) {
  return '/' + key.split('/').map(encodeRfc3986).join('/');
}

export function canonicalQueryString(query) {
  return Object.keys(query)
    .sort()
    .map((k) => `${encodeRfc3986(k)}=${encodeRfc3986(String(query[k]))}`)
    .join('&');
}

export function buildCanonicalRequest({ method, canonicalUri, canonicalQuery, headers, payloadHash }) {
  const lower = {};
  for (const k of Object.keys(headers)) lower[k.toLowerCase()] = String(headers[k]).trim();
  const names = Object.keys(lower).sort();
  const canonicalHeaders = names.map((n) => `${n}:${lower[n]}\n`).join('');
  const signedHeaders = names.join(';');
  const canonicalRequest = [
    method,
    canonicalUri,
    canonicalQuery,
    canonicalHeaders + '\n' + signedHeaders,
    payloadHash,
  ].join('\n');
  return { canonicalRequest, signedHeaders };
}

function amzDateNow() {
  return new Date().toISOString().replace(/[:-]|\.\d{3}/g, '');
}

export async function signRequest(opts) {
  const {
    method, endpoint, canonicalUri, query = {},
    payloadHash = 'UNSIGNED-PAYLOAD', region, service = 's3',
    accessKey, secretKey, sessionToken, amzDate, extraHeaders = {},
  } = opts;

  const host = new URL(endpoint).host;
  const stamp = amzDate || amzDateNow();
  const dateStamp = stamp.slice(0, 8);

  const headers = {
    host,
    'x-amz-content-sha256': payloadHash,
    'x-amz-date': stamp,
    ...extraHeaders,
  };
  if (sessionToken) headers['x-amz-security-token'] = sessionToken;

  const canonicalQuery = canonicalQueryString(query);
  const { canonicalRequest, signedHeaders } = buildCanonicalRequest({
    method, canonicalUri, canonicalQuery, headers, payloadHash,
  });

  const scope = `${dateStamp}/${region}/${service}/aws4_request`;
  const stringToSign = [
    'AWS4-HMAC-SHA256', stamp, scope, await sha256Hex(canonicalRequest),
  ].join('\n');
  const kSigning = await signingKey(secretKey, dateStamp, region, service);
  const signature = toHex(await hmac(kSigning, stringToSign));
  const authorization =
    `AWS4-HMAC-SHA256 Credential=${accessKey}/${scope}, ` +
    `SignedHeaders=${signedHeaders}, Signature=${signature}`;

  // 'host' est signé mais jamais envoyé via fetch (interdit au navigateur).
  const sendHeaders = { ...headers, Authorization: authorization };
  delete sendHeaders.host;

  const qs = canonicalQuery ? `?${canonicalQuery}` : '';
  const url = `${endpoint.replace(/\/$/, '')}${canonicalUri}${qs}`;
  return { url, headers: sendHeaders };
}
```

- [ ] **Step 5 : Lancer le test unitaire et vérifier qu'il passe**

Run: `node --test site-tests/sigv4.test.mjs`
Expected: PASS (3 tests).

- [ ] **Step 6 : Écrire `site-tests/provision.sh` et `site-tests/deprovision.sh` (bucket + clé scopée de test, via `mc`)**

`site-tests/provision.sh` :
```bash
#!/usr/bin/env bash
# Provisionne un bucket jetable + une clé scopée écriture-seule et imprime des exports.
# Usage : eval "$(bash site-tests/provision.sh)"
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
```

`site-tests/deprovision.sh` :
```bash
#!/usr/bin/env bash
set -euo pipefail
ALIAS="${INTAKE_MC_ALIAS:-myminio}"
[ -n "${TEST_SCOPED_AK:-}" ] && mc admin user svcacct rm "$ALIAS" "$TEST_SCOPED_AK" || true
[ -n "${TEST_DATA_BUCKET:-}" ] && mc rb --force "$ALIAS/$TEST_DATA_BUCKET" || true
```

- [ ] **Step 7 : Ajouter à `site-tests/sigv4.test.mjs` un test d'intégration end-to-end (PUT signé réel → MinIO)**

Ajouter à la fin du fichier :

```javascript
import { signRequest } from '../site/sigv4.js';

const BUCKET = process.env.TEST_DATA_BUCKET;
const ENDPOINT = process.env.INTAKE_ENDPOINT || 'http://localhost:9000';
const REGION = process.env.INTAKE_REGION || 'us-east-1';

test('PUT signé réel accepté par MinIO', { skip: !BUCKET }, async () => {
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
  assert.strictEqual(res.status, 200, `PUT status ${res.status} — signature invalide ? ${await res.text()}`);
});
```

- [ ] **Step 8 : Lancer les tests avec un bucket provisionné et vérifier qu'ils passent**

```bash
eval "$(bash site-tests/provision.sh)"
node --test site-tests/sigv4.test.mjs
bash site-tests/deprovision.sh
```
Expected: PASS (unitaires + PUT signé accepté par MinIO — preuve que la signature est correcte de bout en bout).

- [ ] **Step 9 : Commit**

```bash
git add site/package.json site/sigv4.js site-tests/
git commit -m "feat(site): zero-dependency SigV4 signer (Web Crypto) + unit + MinIO integration tests"
```

---

### Task 2 : `upload.js` — upload multipart reprenable + tests

**Files:**
- Create: `site/upload.js`
- Test: `site-tests/upload.test.mjs`

**Interfaces:**
- Consumes: `signRequest`, `encodePath` de `sigv4.js`.
- Produces (exports de `site/upload.js`) :
  - `multipartUpload({endpoint, region, bucket, key, body, creds, partSize?, onProgress?}) => Promise<{uploadId, parts}>`
    - `body` : `Blob`/`File` (navigateur) ou `Uint8Array` (Node) ; `creds = {accessKey, secretKey, sessionToken?}` ; `onProgress({uploaded, total, part, parts})`.
  - `putPartWithRetry({base, uri, uploadId, partNumber, chunk}, attempts) => Promise<string>` (retourne l'ETag)

- [ ] **Step 1 : Écrire le test `site-tests/upload.test.mjs` (retry unitaire + multipart d'intégration)**

```javascript
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
        uri: '/bucket/key', uploadId: 'uid', partNumber: 1, chunk: new Uint8Array([1, 2, 3]),
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
    endpoint: ENDPOINT, region: REGION, bucket: BUCKET, key, body,
    creds: { accessKey: process.env.TEST_SCOPED_AK, secretKey: process.env.TEST_SCOPED_SK },
    onProgress: (p) => { last = p.uploaded; },
  });
  assert.ok(r.parts.length >= 3, `attendu >=3 parts, obtenu ${r.parts.length}`);
  assert.strictEqual(last, size);

  // Vérifier la taille côté serveur via mc (admin).
  const out = execFileSync('mc', ['--json', 'stat', `${ALIAS}/${BUCKET}/${key}`]).toString();
  const stat = JSON.parse(out.trim().split('\n')[0]);
  assert.strictEqual(Number(stat.size), size);
});
```

- [ ] **Step 2 : Lancer le test pour vérifier qu'il échoue**

Run: `node --test site-tests/upload.test.mjs`
Expected: FAIL (module `../site/upload.js` introuvable).

- [ ] **Step 3 : Implémenter `site/upload.js`**

```javascript
// upload.js — upload multipart S3 reprenable, ZÉRO dépendance (fetch + sigv4.js).
import { signRequest, encodePath } from './sigv4.js';

const DEFAULT_PART_SIZE = 8 * 1024 * 1024; // 8 MiB

function parseTag(xml, tag) {
  const m = xml.match(new RegExp(`<${tag}>([^<]*)</${tag}>`));
  return m ? m[1] : null;
}

export async function putPartWithRetry({ base, uri, uploadId, partNumber, chunk }, attempts) {
  let lastErr;
  for (let a = 0; a < attempts; a++) {
    try {
      const signed = await signRequest({
        ...base,
        method: 'PUT',
        canonicalUri: uri,
        query: { partNumber: String(partNumber), uploadId },
        payloadHash: 'UNSIGNED-PAYLOAD',
      });
      const res = await fetch(signed.url, { method: 'PUT', headers: signed.headers, body: chunk });
      if (!res.ok) throw new Error(`part ${partNumber} → ${res.status}: ${await res.text()}`);
      const etag = res.headers.get('ETag');
      if (!etag) throw new Error(`ETag absent pour la part ${partNumber} (CORS expose-headers ?)`);
      return etag;
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr;
}

export async function multipartUpload({
  endpoint, region, bucket, key, body, creds, partSize = DEFAULT_PART_SIZE, onProgress,
}) {
  const size = body.size !== undefined ? body.size : body.byteLength;
  const slice = (s, e) => (body.slice ? body.slice(s, e) : body.subarray(s, e));
  const uri = encodePath(`${bucket}/${key}`);
  const base = {
    endpoint, region,
    accessKey: creds.accessKey, secretKey: creds.secretKey, sessionToken: creds.sessionToken,
  };

  // 1. initier
  const init = await signRequest({
    ...base, method: 'POST', canonicalUri: uri, query: { uploads: '' }, payloadHash: 'UNSIGNED-PAYLOAD',
  });
  const initRes = await fetch(init.url, { method: 'POST', headers: init.headers });
  if (!initRes.ok) throw new Error(`initiate → ${initRes.status}: ${await initRes.text()}`);
  const uploadId = parseTag(await initRes.text(), 'UploadId');
  if (!uploadId) throw new Error('UploadId absent de la réponse CreateMultipartUpload');

  // 2. parts (séquentiel + reprise par part)
  const parts = [];
  let uploaded = 0;
  const nParts = Math.max(1, Math.ceil(size / partSize));
  for (let i = 0; i < nParts; i++) {
    const start = i * partSize;
    const end = Math.min(size, start + partSize);
    const chunk = slice(start, end);
    const partNumber = i + 1;
    const etag = await putPartWithRetry({ base, uri, uploadId, partNumber, chunk }, 3);
    parts.push({ partNumber, etag });
    uploaded += end - start;
    if (onProgress) onProgress({ uploaded, total: size, part: partNumber, parts: nParts });
  }

  // 3. finaliser
  const xml =
    '<CompleteMultipartUpload>' +
    parts.map((p) => `<Part><PartNumber>${p.partNumber}</PartNumber><ETag>${p.etag}</ETag></Part>`).join('') +
    '</CompleteMultipartUpload>';
  const comp = await signRequest({
    ...base, method: 'POST', canonicalUri: uri, query: { uploadId }, payloadHash: 'UNSIGNED-PAYLOAD',
  });
  const compRes = await fetch(comp.url, { method: 'POST', headers: comp.headers, body: xml });
  if (!compRes.ok) throw new Error(`complete → ${compRes.status}: ${await compRes.text()}`);
  return { uploadId, parts };
}
```

- [ ] **Step 4 : Lancer les tests et vérifier qu'ils passent**

```bash
eval "$(bash site-tests/provision.sh)"
node --test site-tests/upload.test.mjs
bash site-tests/deprovision.sh
```
Expected: PASS (retry unitaire + multipart réel de 20 MiB vérifié côté serveur).

- [ ] **Step 5 : Commit**

```bash
git add site/upload.js site-tests/upload.test.mjs
git commit -m "feat(site): resumable multipart upload over fetch + unit/integration tests"
```

---

### Task 3 : `index.html` — UI (credentials, glisser-déposer, progression) + E2E navigateur

**Files:**
- Create: `site/index.html`
- Create: `site/config.example.json`
- Create: `site-tests/MANUAL-E2E.md`

**Interfaces:**
- Consumes: `multipartUpload` de `upload.js` ; `config.json` (déposé par `create_case`, Plan A) contenant `{endpoint, region, dataBucket, usePathStyle}`.

- [ ] **Step 1 : Écrire `site/config.example.json` (forme du fichier généré par affaire)**

```json
{
  "endpoint": "http://localhost:9000",
  "region": "us-east-1",
  "dataBucket": "intake-data-acme-2026-abcd",
  "usePathStyle": true
}
```

- [ ] **Step 2 : Écrire `site/index.html`**

```html
<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Dépôt de fichiers sécurisé</title>
  <style>
    body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; }
    input { display: block; width: 100%; margin: .4rem 0; padding: .5rem; box-sizing: border-box; }
    #drop { border: 2px dashed #999; border-radius: 8px; padding: 2rem; text-align: center; color: #666; margin: 1rem 0; }
    #drop.over { border-color: #333; color: #333; }
    progress { width: 100%; height: 1.2rem; }
    #status { margin-top: 1rem; font-weight: 600; }
    .err { color: #b00; }
  </style>
</head>
<body>
  <h1>Dépôt de fichiers</h1>
  <p>Renseignez les identifiants fournis, puis déposez votre fichier.</p>
  <input id="ak" placeholder="Access key" autocomplete="off" />
  <input id="sk" placeholder="Secret key" type="password" autocomplete="off" />
  <input id="st" placeholder="Session token (laisser vide si non fourni)" autocomplete="off" />
  <input type="file" id="file" />
  <div id="drop">Ou glissez un fichier ici</div>
  <progress id="bar" value="0" max="100"></progress>
  <div id="status"></div>

  <script type="module">
    import { multipartUpload } from './upload.js';

    const $ = (id) => document.getElementById(id);
    let config = null;
    fetch('./config.json').then((r) => r.json()).then((c) => { config = c; })
      .catch(() => { $('status').innerHTML = '<span class="err">config.json introuvable</span>'; });

    // Pré-remplissage via le fragment d'URL (#ak=...&sk=...&st=...)
    const frag = new URLSearchParams(location.hash.slice(1));
    for (const k of ['ak', 'sk', 'st']) if (frag.get(k)) $(k).value = frag.get(k);

    async function start(file) {
      if (!file) return;
      if (!config) { $('status').innerHTML = '<span class="err">config non chargée</span>'; return; }
      const creds = { accessKey: $('ak').value.trim(), secretKey: $('sk').value.trim(), sessionToken: $('st').value.trim() || undefined };
      if (!creds.accessKey || !creds.secretKey) { $('status').innerHTML = '<span class="err">identifiants manquants</span>'; return; }
      const key = `data/${Date.now()}-${file.name}`;
      $('status').textContent = 'Envoi en cours…';
      try {
        await multipartUpload({
          endpoint: config.endpoint, region: config.region, bucket: config.dataBucket,
          key, body: file, creds,
          onProgress: (p) => { $('bar').value = Math.round((100 * p.uploaded) / p.total); },
        });
        $('status').textContent = 'Terminé ✔ — merci, vous pouvez fermer cette page.';
      } catch (e) {
        $('status').innerHTML = `<span class="err">Échec : ${e.message}</span>`;
      }
    }

    $('file').addEventListener('change', (e) => start(e.target.files[0]));
    const drop = $('drop');
    drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
    drop.addEventListener('dragleave', () => drop.classList.remove('over'));
    drop.addEventListener('drop', (e) => {
      e.preventDefault(); drop.classList.remove('over');
      start(e.dataTransfer.files[0]);
    });
  </script>
</body>
</html>
```

- [ ] **Step 3 : Écrire la checklist E2E `site-tests/MANUAL-E2E.md`**

````markdown
# Test E2E manuel (navigateur)

Prérequis : MinIO lancé + bootstrap (Plan A), `intake` construit, `./site/` contient
`index.html`, `sigv4.js`, `upload.js`, `package.json`.

1. Créer une affaire (déploie `./site/` + `config.json`, imprime l'URL + les creds) :
   ```bash
   ./target/release/intake create-case demo-e2e
   ```
2. Ouvrir l'`site_url` imprimée dans un navigateur (sur `localhost` → contexte sécurisé OK).
3. Coller `access_key` / `secret_key` (session token vide), glisser un fichier de
   plusieurs centaines de Mo.
4. Vérifier : la barre progresse, puis « Terminé ✔ ».
5. Côté équipe :
   ```bash
   ./target/release/intake pull-case demo-e2e --dest ./pulled
   ```
   → le fichier déposé est présent et intègre (comparer la taille / un `sha256sum`).
6. Détruire :
   ```bash
   ./target/release/intake teardown-case demo-e2e --yes
   ```

Note : la logique de signature et de multipart est déjà couverte par les tests Node
(`node --test site-tests/`). Ce test manuel ne valide que le câblage UI (glisser-déposer,
barre de progression, chargement de `config.json`).
````

- [ ] **Step 4 : Vérifier le rendu et le câblage manuellement**

Exécuter la checklist `site-tests/MANUAL-E2E.md` de bout en bout.
Expected: upload multi-Mo réussi depuis le navigateur, fichier récupéré intact par `pull-case`.

- [ ] **Step 5 : Commit**

```bash
git add site/index.html site/config.example.json site-tests/MANUAL-E2E.md
git commit -m "feat(site): upload UI (drag-drop + progress) + manual E2E checklist"
```

---

## Auto-revue

**Couverture de la spec (§5.3) :**
- `index.html` + `sigv4.js` + `upload.js`, zéro dépendance, pas de build → Tasks 1–3.
- SigV4 via Web Crypto, canonicalisation testée exactement + contre MinIO → Task 1.
- Multipart `Create/UploadPart/Complete` + `UNSIGNED-PAYLOAD` + reprise + progression → Task 2.
- Saisie des 3 valeurs OU fragment d'URL → Task 3.
- `config.json` non-secret, path-style, même origine (pas de CORS en proto) → Task 3 + Plan A.

**Cohérence des types :** exports de `sigv4.js` (`signRequest`, `encodePath`,
`buildCanonicalRequest`, `sha256Hex`, `signingKey`, `canonicalQueryString`,
`encodeRfc3986`) consommés par `upload.js` (`multipartUpload`, `putPartWithRetry`) et
par `index.html` (`multipartUpload`). Signatures alignées entre tâches et tests.

**Écart connu (identique à Plan A) :** SHA-256 navigateur non praticable pour le
multi-Go (Web Crypto sans hash incrémental → tout en mémoire). Intégrité des gros
fichiers = TLS + versioning + clé écriture-seule + comparaison d'ETag/taille côté
équipe. La vérification par manifeste de hashes reste optionnelle et non implémentée
(décision utilisateur en attente).
```
