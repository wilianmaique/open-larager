import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const script = fileURLToPath(new URL('./release-version.mjs', import.meta.url));
const paths = [
  'package.json',
  'package-lock.json',
  'src-tauri/tauri.conf.json',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
];
const sourceRoot = fileURLToPath(new URL('../', import.meta.url));
const sources = await Promise.all(paths.map((path) => readFile(join(sourceRoot, path), 'utf8')));

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'open-larager-version-'));
  t.after(async () => {
    assert.equal(dirname(resolve(root)).toLowerCase(), resolve(tmpdir()).toLowerCase());
    assert.ok(root.includes('open-larager-version-'));
    await rm(root, { recursive: true, force: true });
  });
  await mkdir(join(root, 'src-tauri'));
  for (const [index, path] of paths.entries()) await writeFile(join(root, path), sources[index]);
  return {
    root,
    run: (...args) =>
      spawnSync(process.execPath, [script, ...args], {
        cwd: root,
        encoding: 'utf8',
        windowsHide: true,
        env: { ...process.env, RELEASE_TAG: '' },
      }),
    read: (path) => readFile(join(root, path), 'utf8'),
  };
}

test('setting a version updates all manifests and lockfiles without changing dependencies', async (t) => {
  const { run, read } = await fixture(t);
  assert.equal(run('set', '1.2.3-rc.1').status, 0);
  const checked = run('check', 'v1.2.3-rc.1');
  assert.equal(checked.status, 0, checked.stderr);
  const pkg = JSON.parse(await read('package.json'));
  const original = JSON.parse(sources[0]);
  assert.deepEqual(pkg.dependencies, original.dependencies);
  assert.deepEqual(pkg.devDependencies, original.devDependencies);
  assert.equal(
    await read('src-tauri/Cargo.lock'),
    sources[4].replace(/(name = "open-larager"\r?\nversion = ")[^"]+/, '$11.2.3-rc.1'),
  );
  assert.equal(
    await read('src-tauri/Cargo.toml'),
    sources[3].replace(/(\[package\][\s\S]*?version = ")[^"]+/, '$11.2.3-rc.1'),
  );
});

test('invalid versions leave every file intact', async (t) => {
  const { run, read } = await fixture(t);
  for (const invalid of ['v0.2.0', '01.2.3', '1.2', '1.2.3-01', '1.2.3+metadata', '../escape']) {
    assert.notEqual(run('set', invalid).status, 0, invalid);
  }
  for (const [index, path] of paths.entries()) assert.equal(await read(path), sources[index]);
});

test('a tag must match the version, including prerelease identifiers', async (t) => {
  const { run } = await fixture(t);
  assert.equal(run('set', '1.2.3-beta.2').status, 0);
  for (const invalid of ['1.2.3-beta.2', 'v1.2.3', 'v1.2.4-beta.2']) {
    assert.notEqual(run('check', invalid).status, 0, invalid);
  }
  assert.equal(run('check', 'v1.2.3-beta.2').status, 0);
});

test('inconsistent Rust and npm lockfile versions fail verification', async (t) => {
  const { run, read, root } = await fixture(t);
  await writeFile(
    join(root, 'src-tauri/Cargo.lock'),
    sources[4].replace(/(name = "open-larager"\r?\nversion = ")[^"]+/, '$19.9.9'),
  );
  assert.notEqual(run('check').status, 0);
  assert.equal(run('set', '1.2.3').status, 0);
  const npm = JSON.parse(await read('package-lock.json'));
  npm.packages[''].version = '9.9.9';
  await writeFile(join(root, 'package-lock.json'), JSON.stringify(npm));
  assert.notEqual(run('check').status, 0);
});

test('a missing root package is detected before any version is changed', async (t) => {
  const { run, read, root } = await fixture(t);
  await writeFile(join(root, 'src-tauri/Cargo.lock'), 'version = 4\n');
  assert.notEqual(run('set', '1.2.3').status, 0);
  assert.equal(await read('package.json'), sources[0]);
  assert.equal(await read('src-tauri/Cargo.toml'), sources[3]);
});

test('a missing Cargo package version never falls through to a dependency version', async (t) => {
  const { run, read, root } = await fixture(t);
  const cargo = '[package]\nname = "open-larager"\n\n[dependencies.example]\nversion = "1.0.0"\n';
  await writeFile(join(root, 'src-tauri/Cargo.toml'), cargo);
  assert.notEqual(run('set', '1.2.3').status, 0);
  assert.equal(await read('src-tauri/Cargo.toml'), cargo);
  assert.equal(await read('package.json'), sources[0]);
});
