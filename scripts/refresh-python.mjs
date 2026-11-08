// Full CPython runtime ZIPs, selected from the publisher's install-manager index.
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

export async function refreshPython({ json, add }) {
  const index = await json('https://www.python.org/ftp/python/index-windows.json');
  const stable = index.versions
    .filter(
      (release) =>
        release.schema === 1 &&
        release.company === 'PythonCore' &&
        /^pythoncore-3\.\d+-64$/.test(release.id) &&
        /^3\.\d+\.\d+$/.test(release['sort-version']),
    )
    .sort((a, b) => b['sort-version'].localeCompare(a['sort-version'], 'en', { numeric: true }));
  const branches = new Set();
  for (const release of stable) {
    const version = release['sort-version'];
    const branch = version.split('.').slice(0, 2).join('.');
    if (branches.has(branch)) continue;
    if (release.url !== `https://www.python.org/ftp/python/${version}/python-${version}-amd64.zip`)
      throw new Error(`Unexpected Python runtime URL: ${release.url}`);
    if (!/^[a-f0-9]{64}$/i.test(release.hash?.sha256 ?? ''))
      throw new Error(`Missing Python publisher SHA-256: ${version}`);
    await add({
      service: 'python',
      version,
      url: release.url,
      sha256: release.hash.sha256,
      archiveRoot: '',
      format: 'zip',
      source: 'Python Software Foundation',
      sourceUrl: 'https://www.python.org/downloads/windows/',
      verification: 'publisher-sha256',
    });
    branches.add(branch);
    if (branches.size === 3) break;
  }
  if (branches.size < 2) throw new Error('Python index lacks two stable x64 runtime branches');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const output = new URL('../src-tauri/catalog/packages.json', import.meta.url);
  const catalog = JSON.parse(await readFile(output, 'utf8'));
  const request = async (url, method = 'GET') => {
    const response = await fetch(url, { method, signal: AbortSignal.timeout(60_000) });
    if (!response.ok) throw new Error(`${response.status}: ${url}`);
    return response;
  };
  const packages = [];
  await refreshPython({
    json: async (url) => (await request(url)).json(),
    add: async (pkg) => {
      const response = await request(pkg.url, 'HEAD');
      packages.push({
        community: false,
        size: Number(response.headers.get('content-length')),
        ...pkg,
      });
      console.log(`${pkg.service} ${pkg.version} ${pkg.sha256.slice(0, 12)}`);
    },
  });
  catalog.packages = [...catalog.packages.filter((pkg) => pkg.service !== 'python'), ...packages];
  catalog.generatedAt = new Date().toISOString();
  await writeFile(output, `${JSON.stringify(catalog, null, 2)}\n`);
}
