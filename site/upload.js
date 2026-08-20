// upload.js — resumable S3 multipart upload, ZERO dependencies (fetch + sigv4.js).
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
      if (!res.ok) throw new Error(`part ${partNumber} -> ${res.status}: ${await res.text()}`);
      const etag = res.headers.get('ETag');
      if (!etag) throw new Error(`missing ETag for part ${partNumber} (CORS expose-headers?)`);
      return etag;
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr;
}

export async function multipartUpload({
  endpoint,
  region,
  bucket,
  key,
  body,
  creds,
  partSize = DEFAULT_PART_SIZE,
  onProgress,
}) {
  const size = body.size !== undefined ? body.size : body.byteLength;
  const slice = (s, e) => (body.slice ? body.slice(s, e) : body.subarray(s, e));
  const uri = encodePath(`${bucket}/${key}`);
  const base = {
    endpoint,
    region,
    accessKey: creds.accessKey,
    secretKey: creds.secretKey,
    sessionToken: creds.sessionToken,
  };

  // 1. initiate
  const init = await signRequest({
    ...base,
    method: 'POST',
    canonicalUri: uri,
    query: { uploads: '' },
    payloadHash: 'UNSIGNED-PAYLOAD',
  });
  const initRes = await fetch(init.url, { method: 'POST', headers: init.headers });
  if (!initRes.ok) throw new Error(`initiate -> ${initRes.status}: ${await initRes.text()}`);
  const uploadId = parseTag(await initRes.text(), 'UploadId');
  if (!uploadId) throw new Error('UploadId missing from CreateMultipartUpload response');

  // 2. parts (sequential + per-part retry)
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

  // 3. complete
  const xml =
    '<CompleteMultipartUpload>' +
    parts
      .map((p) => `<Part><PartNumber>${p.partNumber}</PartNumber><ETag>${p.etag}</ETag></Part>`)
      .join('') +
    '</CompleteMultipartUpload>';
  const comp = await signRequest({
    ...base,
    method: 'POST',
    canonicalUri: uri,
    query: { uploadId },
    payloadHash: 'UNSIGNED-PAYLOAD',
  });
  const compRes = await fetch(comp.url, { method: 'POST', headers: comp.headers, body: xml });
  if (!compRes.ok) throw new Error(`complete -> ${compRes.status}: ${await compRes.text()}`);
  return { uploadId, parts };
}
