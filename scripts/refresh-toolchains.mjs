// Shared by the full catalog refresh and the focused toolchain refresh.
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

export async function refreshToolchains({ json, text, add }) {
  const releases = await json('https://nodejs.org/dist/index.json');
  const compatible = releases.filter((release) => release.files.includes('win-x64-zip'));
  const ltsMajors = [
    ...new Set(compatible.filter((r) => r.lts).map((r) => r.version.split('.')[0])),
  ].slice(0, 2);
  const selected = ltsMajors.map((major) =>
    compatible.find((r) => r.lts && r.version.startsWith(`${major}.`)),
  );
  if (!selected.some((r) => r.version === compatible[0].version)) selected.push(compatible[0]);
  for (const release of selected) {
    const filename = `node-${release.version}-win-x64.zip`;
    const sums = await text(`https://nodejs.org/dist/${release.version}/SHASUMS256.txt`);
    const sha256 = sums
      .split('\n')
      .find((line) => line.trim().endsWith(`  ${filename}`))
      ?.split(/\s+/)[0];
    if (!sha256) throw new Error(`Missing Node.js publisher checksum: ${filename}`);
    await add({
      service: 'nodejs',
      version: release.version.slice(1),
      url: `https://nodejs.org/dist/${release.version}/${filename}`,
      sha256,
      archiveRoot: filename.slice(0, -4),
      format: 'zip',
      bundledTools: { npm: release.npm },
      source: 'Node.js',
      sourceUrl: 'https://nodejs.org/en/download',
      verification: 'publisher-sha256',
    });
  }
  const versions = await json('https://getcomposer.org/versions');
  const composer = versions.stable.find((release) => !release.lts && !release.eol);
  if (!composer || !/^\/download\/\d+\.\d+\.\d+\/composer\.phar$/.test(composer.path))
    throw new Error('Composer stable feed changed');
  const url = `https://getcomposer.org${composer.path}`;
  await add({
    service: 'composer',
    version: composer.version,
    url,
    sha256: (await text(`${url}.sha256sum`)).trim().split(/\s+/)[0],
    archiveRoot: '',
    format: 'phar',
    source: 'Composer',
    sourceUrl: 'https://getcomposer.org/download/',
    verification: 'publisher-sha256',
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const output = new URL('../src-tauri/catalog/packages.json', import.meta.url);
  const catalog = JSON.parse(await readFile(output, 'utf8'));
  const packages = [];
  const request = async (url) => {
    const response = await fetch(url, { signal: AbortSignal.timeout(60_000) });
    if (!response.ok) throw new Error(`${response.status}: ${url}`);
    return response;
  };
  await refreshToolchains({
    json: async (url) => (await request(url)).json(),
    text: async (url) => (await request(url)).text(),
    add: async (pkg) => {
      if (!/^[a-f0-9]{64}$/i.test(pkg.sha256)) throw new Error('Invalid publisher SHA-256');
      const response = await fetch(pkg.url, {
        method: 'HEAD',
        signal: AbortSignal.timeout(60_000),
      });
      if (!response.ok) throw new Error(`${response.status}: ${pkg.url}`);
      packages.push({
        community: false,
        size: Number(response.headers.get('content-length') ?? 0),
        ...pkg,
      });
      console.log(`${pkg.service} ${pkg.version} ${pkg.sha256.slice(0, 12)}`);
    },
  });
  catalog.packages = [
    ...catalog.packages.filter((pkg) => !['nodejs', 'composer'].includes(pkg.service)),
    ...packages,
  ];
  catalog.generatedAt = new Date().toISOString();
  await writeFile(output, `${JSON.stringify(catalog, null, 2)}\n`);
}
