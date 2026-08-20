// sigv4.js — AWS SigV4 signing, ZERO dependencies (Web Crypto).
// Crypto primitives = crypto.subtle (browser/Node). We only implement the
// canonicalization (string formatting), tested exactly + against MinIO.

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
    'raw',
    keyBytes,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign']
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
    /[!'()*]/g,
    (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase()
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
    method,
    endpoint,
    canonicalUri,
    query = {},
    payloadHash = 'UNSIGNED-PAYLOAD',
    region,
    service = 's3',
    accessKey,
    secretKey,
    sessionToken,
    amzDate,
    extraHeaders = {},
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
    method,
    canonicalUri,
    canonicalQuery,
    headers,
    payloadHash,
  });

  const scope = `${dateStamp}/${region}/${service}/aws4_request`;
  const stringToSign = [
    'AWS4-HMAC-SHA256',
    stamp,
    scope,
    await sha256Hex(canonicalRequest),
  ].join('\n');
  const kSigning = await signingKey(secretKey, dateStamp, region, service);
  const signature = toHex(await hmac(kSigning, stringToSign));
  const authorization =
    `AWS4-HMAC-SHA256 Credential=${accessKey}/${scope}, ` +
    `SignedHeaders=${signedHeaders}, Signature=${signature}`;

  // 'host' is signed but never sent via fetch (forbidden in the browser).
  const sendHeaders = { ...headers, Authorization: authorization };
  delete sendHeaders.host;

  const qs = canonicalQuery ? `?${canonicalQuery}` : '';
  const url = `${endpoint.replace(/\/$/, '')}${canonicalUri}${qs}`;
  return { url, headers: sendHeaders };
}
