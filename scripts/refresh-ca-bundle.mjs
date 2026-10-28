// Maintainer-only: runtime trust never downloads unreviewed CA bundles.
import assert from 'node:assert/strict';
import { createHash, X509Certificate } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';

const directory = new URL('../src-tauri/certificates/', import.meta.url);
const target = new URL('mozilla.pem', directory);
const metadata = new URL('../src-tauri/catalog/ca-bundle.json', import.meta.url);
function inspect(bytes) {
  const certificates =
    bytes.toString('utf8').match(/-----BEGIN CERTIFICATE-----[\s\S]+?-----END CERTIFICATE-----/g) ??
    [];
  assert.ok(certificates.length >= 100, 'Incomplete Mozilla CA bundle');
  for (const certificate of certificates) new X509Certificate(certificate);
  return certificates.length;
}
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
if (process.argv.includes('--check')) {
  const record = JSON.parse(await readFile(metadata, 'utf8'));
  const bytes = await readFile(target);
  assert.equal(sha256(bytes), record.sha256);
  assert.equal(bytes.length, record.size);
  assert.equal(inspect(bytes), record.certificates);
  console.log(
    `Verified Mozilla CA bundle ${record.revision}: ${record.certificates} certificates.`,
  );
} else {
  async function request(url) {
    const parsed = new URL(url);
    assert.equal(parsed.protocol, 'https:');
    assert.equal(parsed.hostname, 'curl.se');
    const response = await fetch(url, { redirect: 'error', signal: AbortSignal.timeout(60_000) });
    if (!response.ok) throw new Error(`${response.status}: ${url}`);
    return response;
  }
  const page = await (await request('https://curl.se/docs/caextract.html')).text();
  const revision = page.match(/cacert-(\d{4}-\d{2}-\d{2})\.pem/)?.[1];
  assert.ok(revision, 'Missing versioned CA bundle');
  const url = `https://curl.se/ca/cacert-${revision}.pem`;
  const [bytes, checksum] = await Promise.all([
    request(url).then(async (response) => Buffer.from(await response.arrayBuffer())),
    request(`${url}.sha256`).then((response) => response.text()),
  ]);
  const digest = checksum.match(/\b[a-f0-9]{64}\b/i)?.[0]?.toLowerCase();
  assert.equal(sha256(bytes), digest, 'Upstream SHA-256 mismatch');
  const record = {
    revision,
    url,
    sha256: digest,
    size: bytes.length,
    certificates: inspect(bytes),
    license: 'MPL-2.0',
    source: 'https://curl.se/docs/caextract.html',
  };
  await mkdir(directory, { recursive: true });
  await writeFile(target, bytes);
  await writeFile(metadata, JSON.stringify(record, null, 2) + '\n');
  console.log(
    `Pinned Mozilla CA bundle ${revision}: ${record.certificates} certificates, ${digest}. Review before release.`,
  );
}
