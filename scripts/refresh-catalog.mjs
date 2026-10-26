// Maintainer tool. Runtime installation only accepts the checked-in, pinned catalog.
import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const headers = { 'User-Agent': 'OpenLarager-catalog/0.1', Accept: 'application/vnd.github+json' };
async function request(url) {
  const response = await fetch(url, { headers, signal: AbortSignal.timeout(300_000) });
  if (!response.ok) throw new Error(`${response.status}: ${url}`);
  return response;
}
const text = async (url) => (await request(url)).text();
const json = async (url) => (await request(url)).json();
const packages = [];

async function add(pkg) {
  if (!pkg.sha256) {
    console.log(`Pinning official HTTPS artifact: ${pkg.service} ${pkg.version}`);
    const response = await request(pkg.url);
    const hash = createHash('sha256');
    let size = 0;
    for await (const chunk of response.body) {
      hash.update(chunk);
      size += chunk.length;
    }
    pkg.sha256 = hash.digest('hex');
    pkg.size = size;
    pkg.verification = 'https-pinned';
  }
  if (!/^[a-f0-9]{64}$/i.test(pkg.sha256)) throw new Error(`Invalid SHA-256: ${pkg.service}`);
  packages.push({ community: false, size: 0, ...pkg, sha256: pkg.sha256.toLowerCase() });
  console.log(`${pkg.service} ${pkg.version} ${pkg.sha256.slice(0, 12)}`);
}

const php = await json('https://downloads.php.net/~windows/releases/releases.json');
for (const branch of ['8.5', '8.4', '8.3', '8.2']) {
  const release = php[branch];
  if (!release) continue;
  const build = Object.entries(release).find(([key]) => /^nts-.+-x64$/.test(key))?.[1]?.zip;
  if (build)
    await add({
      service: 'php',
      version: release.version,
      url: `https://downloads.php.net/~windows/releases/${build.path}`,
      sha256: build.sha256,
      archiveRoot: '',
      source: 'PHP',
      sourceUrl: 'https://www.php.net/downloads.php?os=windows',
      verification: 'publisher-sha256',
      size: Math.round(parseFloat(build.size) * 1024 * 1024),
    });
}

const apachePage = await text('https://www.apachelounge.com/download/');
const apachePath = apachePage.match(/href="([^" ]+httpd-([\d.]+)-[^" ]+-Win64-[^" ]+\.zip)"/i);
if (!apachePath) throw new Error('Apache Lounge page changed');
const apacheUrl = new URL(apachePath[1], 'https://www.apachelounge.com').href;
const apacheSums = await text(`${apacheUrl}.txt`);
const apacheHash =
  apacheSums.match(/(?:SHA2?-?256|SHA256)[^a-f\d]*([a-f\d]{64})/i)?.[1] ??
  apacheSums.match(/\b[a-f\d]{64}\b/i)?.[0];
if (!apacheHash) throw new Error('Apache publisher SHA-256 missing');
await add({
  service: 'apache',
  version: apachePath[2],
  url: apacheUrl,
  sha256: apacheHash,
  archiveRoot: 'Apache24',
  source: 'Apache Lounge',
  sourceUrl: 'https://www.apachelounge.com/download/',
  verification: 'publisher-sha256',
});

const nginxPage = await text('https://nginx.org/en/download.html');
const nginxVersions = [...nginxPage.matchAll(/href="\/download\/nginx-([\d.]+)\.zip"/g)].slice(
  0,
  2,
);
for (const [, version] of nginxVersions)
  await add({
    service: 'nginx',
    version,
    url: `https://nginx.org/download/nginx-${version}.zip`,
    archiveRoot: `nginx-${version}`,
    source: 'NGINX',
    sourceUrl: 'https://nginx.org/en/download.html',
  });

for (const [service, repo, pattern, archiveRoot, community] of [
  ['mailpit', 'axllent/mailpit', /mailpit-windows-amd64\.zip$/, '', false],
  ['caddy', 'caddyserver/caddy', /_windows_amd64\.zip$/, '', false],
  ['redis', 'redis-windows/redis-windows', /Windows-x64-msys2\.zip$/, '', true],
]) {
  const releases = await json(`https://api.github.com/repos/${repo}/releases?per_page=8`);
  let count = 0;
  for (const release of releases.filter((r) => !r.prerelease && !r.draft)) {
    const asset = release.assets.find((a) => pattern.test(a.name));
    if (!asset) continue;
    let sha256 = asset.digest?.replace(/^sha256:/, '');
    if (!sha256) {
      const checksumAsset = release.assets.find((a) => /checksums?\.txt$/i.test(a.name));
      if (checksumAsset) {
        const sums = await text(checksumAsset.browser_download_url);
        sha256 = sums
          .split('\n')
          .find((line) => line.includes(asset.name))
          ?.match(/[a-f\d]{64}/i)?.[0];
      }
    }
    if (!sha256) throw new Error(`Publisher SHA-256 missing: ${repo}/${asset.name}`);
    await add({
      service,
      version: release.tag_name.replace(/^v/, ''),
      url: asset.browser_download_url,
      sha256,
      archiveRoot: service === 'redis' ? asset.name.replace(/\.zip$/, '') : archiveRoot,
      source: repo,
      sourceUrl: `https://github.com/${repo}`,
      size: asset.size,
      community,
      verification: 'publisher-sha256',
    });
    if (++count >= 2) break;
  }
  if (!count) throw new Error(`No compatible Windows asset: ${repo}`);
}

const pgPage = await text('https://www.enterprisedb.com/download-postgresql-binaries');
const pgLinks = [
  ...pgPage.matchAll(
    /href="(https:\/\/sbp\.enterprisedb\.com\/getfile\.jsp\?fileid=\d+)"[^>]*><img alt="Windows x86-64"/g,
  ),
].slice(0, 2);
if (!pgLinks.length) throw new Error('EDB page changed');
for (const [, link] of pgLinks) {
  const resolved = await fetch(link, { method: 'HEAD', headers });
  if (!resolved.ok || !resolved.url.startsWith('https://get.enterprisedb.com/postgresql/'))
    throw new Error('Unexpected EDB redirect');
  const url = resolved.url;
  const version = url.match(/postgresql-([\d.]+-\d+)-windows/)[1];
  await add({
    service: 'postgresql',
    version,
    url,
    archiveRoot: 'pgsql',
    source: 'EDB / PostgreSQL',
    sourceUrl: 'https://www.postgresql.org/download/windows/',
  });
}

// Prefer LTS; never mix initialized database directories between versions.
const mysqlPage = await text('https://dev.mysql.com/downloads/mysql/?os=3&version=8.4');
const mysqlVersion = mysqlPage.match(/mysql-([\d.]+)-winx64\.zip/)?.[1];
if (!mysqlVersion) throw new Error('MySQL download page changed');
await add({
  service: 'mysql',
  version: mysqlVersion,
  url: `https://cdn.mysql.com/Downloads/MySQL-${mysqlVersion.split('.').slice(0, 2).join('.')}/mysql-${mysqlVersion}-winx64.zip`,
  archiveRoot: `mysql-${mysqlVersion}-winx64`,
  source: 'Oracle / MySQL',
  sourceUrl: 'https://dev.mysql.com/downloads/mysql/',
});

const output = fileURLToPath(new URL('../src-tauri/catalog/packages.json', import.meta.url));
await mkdir(dirname(output), { recursive: true });
await writeFile(
  output,
  `${JSON.stringify({ generatedAt: new Date().toISOString(), architecture: 'windows-x86_64', packages }, null, 2)}\n`,
);
console.log(`Saved ${packages.length} pinned packages.`);
