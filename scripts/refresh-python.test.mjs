import assert from 'node:assert/strict';
import test from 'node:test';
import { refreshPython } from './refresh-python.mjs';

const release = (version, id = `pythoncore-${version.split('.').slice(0, 2).join('.')}-64`) => ({
  schema: 1,
  company: 'PythonCore',
  id,
  'sort-version': version,
  url: `https://www.python.org/ftp/python/${version}/python-${version}-amd64.zip`,
  hash: { sha256: 'a'.repeat(64) },
});

test('selects the latest full stable x64 runtime per branch, excluding prereleases and variants', async () => {
  const packages = [];
  await refreshPython({
    json: async () => ({
      versions: [
        release('3.14.9'),
        release('3.15.2'),
        release('3.16.0a1'),
        release('3.14.10'),
        release('3.13.17'),
        release('3.12.10'),
        release('3.15.2', 'pythoncore-3.15t-64'),
        { ...release('3.15.2'), company: 'PythonEmbed' },
      ],
    }),
    add: async (pkg) => packages.push(pkg),
  });
  assert.deepEqual(
    packages.map((pkg) => pkg.version),
    ['3.15.2', '3.14.10', '3.13.17'],
  );
  assert.ok(
    packages.every((pkg) => pkg.verification === 'publisher-sha256' && pkg.archiveRoot === ''),
  );
});

test('rejects a missing checksum or unexpected origin instead of producing a catalog', async () => {
  for (const invalid of [
    { ...release('3.15.2'), hash: {} },
    { ...release('3.15.2'), url: 'https://example.com/python.zip' },
  ]) {
    await assert.rejects(
      refreshPython({
        json: async () => ({ versions: [invalid] }),
        add: async () => assert.fail('Unverified artifact'),
      }),
    );
  }
});
