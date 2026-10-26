// Exercises a real packaged Tauri app and WebView2, in an isolated portable workspace.
// Does not invoke the hosts helper or change the Windows certificate store.
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, copyFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, dirname, basename } from 'node:path';
import { createServer } from 'node:net';
import { chromium, expect } from '@playwright/test';

if (process.platform !== 'win32') throw new Error('This check requires Windows and WebView2.');
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function freePort() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}
const source = resolve(process.argv[2] ?? 'src-tauri/target/release/open-larager.exe');
const screenshot = process.argv[3];
const root = await mkdtemp(join(tmpdir(), 'open-larager-desktop-'));
assert.equal(dirname(resolve(root)).toLowerCase(), resolve(tmpdir()).toLowerCase());
assert.ok(basename(root).startsWith('open-larager-desktop-'));
const executable = join(root, 'open-larager.exe');
await copyFile(source, executable);
await writeFile(join(root, 'larager.portable'), '');
const debuggingPort = await freePort();
const child = spawn(executable, [], {
  cwd: root,
  windowsHide: true,
  stdio: 'ignore',
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debuggingPort} --remote-debugging-address=127.0.0.1`,
  },
});
let exited = false;
let spawnError;
child.once('exit', () => {
  exited = true;
});
child.once('error', (error) => {
  spawnError = error;
});
let browser;
try {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (spawnError) throw spawnError;
    if (exited)
      throw new Error('The packaged application exited before its WebView became available.');
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${debuggingPort}`, {
        timeout: 1000,
      });
      break;
    } catch {
      await delay(250);
    }
  }
  assert.ok(browser, 'WebView2 debugging endpoint did not become available');
  const context = browser.contexts()[0];
  let page;
  for (let attempt = 0; attempt < 40; attempt++) {
    page = context.pages().find((page) => !page.url().startsWith('devtools:'));
    if (page) break;
    await delay(250);
  }
  assert.ok(page, 'No WebView page was created');
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await expect(page.locator('.service-row')).toHaveCount(8, { timeout: 20_000 });
  const invoke = (command, args = {}) =>
    page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), {
      command,
      args,
    });
  const initial = await invoke('get_snapshot');
  assert.equal(initial.services.filter((service) => service.version).length, 0);
  assert.equal(resolve(initial.root).toLowerCase(), resolve(root, 'larager-data').toLowerCase());
  assert.equal(initial.certificate.available, false);
  const smtpPort = await freePort();
  let httpPort = await freePort();
  while (httpPort === smtpPort) httpPort = await freePort();
  await invoke('save_service', {
    id: 'mailpit',
    version: null,
    ports: [smtpPort, httpPort],
    autoStart: false,
  });
  await page.locator('[data-testid="service-mailpit"] .row-actions .button').click();
  const drawer = page.locator('.service-drawer');
  await drawer.locator('.install-select .button').click();
  await expect(drawer.locator('.status-stopped')).toBeVisible({ timeout: 120_000 });
  await drawer.locator('.drawer-status-actions .button').click();
  await expect(drawer.locator('.status-running')).toBeVisible({ timeout: 15_000 });
  const response = await fetch(`http://127.0.0.1:${httpPort}/api/v1/messages`);
  assert.equal(response.ok, true);
  await drawer.locator('.drawer-status-actions .button').click();
  await expect(drawer.locator('.status-stopped')).toBeVisible({ timeout: 15_000 });
  await drawer.locator('[role="tab"]').nth(1).click();
  await expect(drawer.locator('.monaco-editor')).toBeVisible({ timeout: 20_000 });
  await drawer.locator('.drawer-header .icon-button').click();
  if (screenshot) {
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.waitForFunction(() => window.scrollY === 0);
    await page.screenshot({ path: resolve(screenshot), fullPage: true });
  }
  assert.deepEqual(errors, []);
  const powershell = join(
    process.env.SystemRoot ?? 'C:\\Windows',
    'System32/WindowsPowerShell/v1.0/powershell.exe',
  );
  await promisify(execFile)(
    powershell,
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `(Get-Process -Id ${child.pid}).CloseMainWindow() | Out-Null`,
    ],
    { windowsHide: true },
  );
  for (let attempt = 0; attempt < 60 && !exited; attempt++) await delay(250);
  assert.equal(exited, true, 'The main window did not shut down cleanly');
  console.log(
    'Verified packaged WebView2, real IPC, empty first run, portable storage, Mailpit download/start/stop, local Monaco, and application shutdown.',
  );
} finally {
  if (!exited) {
    child.kill();
    await delay(1000);
  }
  await browser?.close().catch(() => {});
  await rm(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
}
