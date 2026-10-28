// Keep the application version and both dependency lockfiles in sync without resolving dependencies.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';

const number = '(?:0|[1-9]\\d*)';
const identifier = '(?:0|[1-9]\\d*|\\d*[a-zA-Z-][0-9a-zA-Z-]*)';
const versionPattern = new RegExp(
  `^${number}\\.${number}\\.${number}(?:-${identifier}(?:\\.${identifier})*)?$`,
);
const [command, argument] = process.argv.slice(2);
assert.ok(
  ['set', 'check'].includes(command),
  'Usage: release-version.mjs set VERSION | check [TAG]',
);

const paths = [
  'package.json',
  'package-lock.json',
  'src-tauri/tauri.conf.json',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
];
const sources = await Promise.all(paths.map((path) => readFile(path, 'utf8')));
const [pkg, npmLock, tauri] = sources.slice(0, 3).map((source) => JSON.parse(source));
// Match only the root package, preserving versions of every dependency and other TOML sections.
const cargoPattern = /^(\[package\]\r?\n(?:(?!^\[)[\s\S])*?^version[ \t]*=[ \t]*")([^"]+)(")/m;
const lockPattern = /(\[\[package\]\]\r?\nname = "open-larager"\r?\nversion = ")([^"]+)(")/;
const cargo = sources[3].match(cargoPattern);
const cargoLock = sources[4].match(lockPattern);
assert.ok(cargo, 'Cannot find [package].version in src-tauri/Cargo.toml');
assert.ok(cargoLock, 'Cannot find open-larager in src-tauri/Cargo.lock');
assert.ok(npmLock.packages?.[''], 'Cannot find the root package in package-lock.json');

const version = command === 'set' ? argument : pkg.version;
assert.ok(versionPattern.test(version ?? ''), 'Use a version such as 0.2.0 or 0.2.0-rc.1');

if (command === 'set') {
  pkg.version = npmLock.version = npmLock.packages[''].version = tauri.version = version;
  const output = [
    ...[pkg, npmLock, tauri].map((value) => `${JSON.stringify(value, null, 2)}\n`),
    sources[3].replace(
      cargoPattern,
      (_, before, _previous, after) => `${before}${version}${after}`,
    ),
    sources[4].replace(lockPattern, (_, before, _previous, after) => `${before}${version}${after}`),
  ];
  // Validate every input before changing any file.
  for (const [index, path] of paths.entries()) await writeFile(path, output[index]);
  console.log(
    `Version set to ${version}. Review and commit the five updated files before tagging.`,
  );
} else {
  const versions = [pkg.version, npmLock.version, tauri.version, cargo[2], cargoLock[2]];
  for (const [index, actual] of versions.entries())
    assert.equal(actual, version, `Version mismatch in ${paths[index]}`);
  assert.equal(
    npmLock.packages[''].version,
    version,
    'Version mismatch in package-lock.json packages',
  );
  const tag = argument ?? process.env.RELEASE_TAG;
  if (tag) assert.equal(tag, `v${version}`, 'Tag must be v followed by the application version');
  console.log(`Verified application version ${version}${tag ? ` and tag ${tag}` : ''}.`);
}
