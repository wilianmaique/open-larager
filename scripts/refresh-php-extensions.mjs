// Maintainer-only: the app installs exclusively from this checked-in catalog.
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';

const target = new URL('../src-tauri/catalog/php-extensions.json', import.meta.url);
const catalog = JSON.parse(await readFile(target, 'utf8'));
const packages = JSON.parse(
  await readFile(new URL('../src-tauri/catalog/packages.json', import.meta.url), 'utf8'),
).packages;
const artifacts = [];
async function request(url) {
  const parsed = new URL(url);
  if (parsed.protocol !== 'https:' || parsed.hostname !== 'downloads.php.net')
    throw new Error('Untrusted extension origin');
  const response = await fetch(url, { redirect: 'error', signal: AbortSignal.timeout(120_000) });
  if (!response.ok) throw new Error(`${response.status}: ${url}`);
  return response;
}
for (const extension of catalog.extensions.filter((extension) => extension.source === 'pecl')) {
  const root = `https://downloads.php.net/~windows/pecl/releases/${extension.id}/`;
  const index = await (await request(root)).text();
  const versions = [...index.matchAll(/href="(\d+\.\d+\.\d+)\/"/g)].map((match) => match[1]);
  versions.sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
  const release = versions[0];
  if (!release) throw new Error(`No stable release: ${extension.id}`);
  const releaseUrl = `${root}${release}/`;
  const builds = await (await request(releaseUrl)).text();
  for (const php of packages.filter((pkg) => pkg.service === 'php')) {
    const build = php.url.match(/-(nts|ts)-Win32-(vs\d+)-(x64)\.zip$/);
    if (!build) throw new Error(`Unrecognized PHP build: ${php.url}`);
    const phpVersion = php.version.split('.').slice(0, 2).join('.');
    const file = `php_${extension.id}-${release}-${phpVersion}-${build[1]}-${build[2]}-${build[3]}.zip`;
    if (!builds.includes(`href="${file}"`)) {
      console.log(`Unavailable: ${extension.id} PHP ${phpVersion}`);
      continue;
    }
    const url = `${releaseUrl}${file}`;
    const bytes = Buffer.from(await (await request(url)).arrayBuffer());
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    artifacts.push({
      id: extension.id,
      version: release,
      phpVersion,
      threadSafe: build[1] === 'ts',
      compiler: build[2],
      architecture: build[3],
      url,
      sha256,
      size: bytes.length,
      verification: 'https-pinned',
    });
    console.log(`${extension.id} ${release} PHP ${phpVersion} ${sha256.slice(0, 12)}`);
  }
}
await writeFile(
  target,
  JSON.stringify({ extensions: catalog.extensions, artifacts }, null, 2) + '\n',
);
