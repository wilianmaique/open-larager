// Exercises a real packaged Tauri app and WebView2, in an isolated portable workspace.
// Does not invoke the hosts helper or change the Windows certificate store.
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import {
  mkdtemp,
  mkdir,
  copyFile,
  writeFile,
  readFile,
  rm,
  readdir,
  realpath,
} from 'node:fs/promises';
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
const smokeOnly = process.argv.includes('--smoke');
const args = process.argv.slice(2).filter((argument) => argument !== '--smoke');
const source = resolve(args[0] ?? 'src-tauri/target/release/open-larager.exe');
const screenshot = args[1];
// Windows TEMP can contain an 8.3 alias while the application returns canonical paths.
const temporaryRoot = await realpath(tmpdir());
const root = await mkdtemp(join(temporaryRoot, 'open-larager-desktop-'));
assert.equal(dirname(resolve(root)).toLowerCase(), resolve(temporaryRoot).toLowerCase());
assert.ok(basename(root).startsWith('open-larager-desktop-'));
const executable = join(root, 'open-larager.exe');
await copyFile(source, executable);
await writeFile(join(root, 'larager.portable'), '');
const debuggingPort = await freePort();
const child = spawn(executable, [], {
  cwd: root,
  windowsHide: true,
  stdio: ['ignore', 'inherit', 'inherit'],
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debuggingPort} --remote-debugging-address=127.0.0.1`,
  },
});
let exited = false;
let exitCode;
let exitSignal;
let spawnError;
child.once('exit', (code, signal) => {
  exited = true;
  exitCode = code;
  exitSignal = signal;
});
child.once('error', (error) => {
  spawnError = error;
});
let browser;
try {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (spawnError) throw spawnError;
    if (exited)
      throw new Error(
        `The packaged application exited before its WebView became available (exit code: ${exitCode}, signal: ${exitSignal ?? 'none'}). Check the application output above. Run this check in a standard Windows session with WebView2 installed.`,
      );
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
  await expect(page.getByTestId('service-mailpit')).toBeVisible({ timeout: 20_000 });
  const invoke = (command, args = {}) =>
    page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), {
      command,
      args,
    });
  const initial = await invoke('get_snapshot');
  await expect(page.locator('.service-row')).toHaveCount(initial.services.length);
  assert.equal(initial.services.filter((service) => service.version).length, 0);
  assert.equal(resolve(initial.root).toLowerCase(), resolve(root, 'larager-data').toLowerCase());
  assert.equal(initial.certificate.available, false);
  assert.equal((await readdir(join(root, 'larager-data'))).includes('EBWebView'), false);
  assert.equal((await readdir(join(root, 'larager-webview'))).includes('EBWebView'), true);
  await invoke('save_preferences', { language: 'en', theme: 'light' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  if (!smokeOnly) {
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
  }
  if (screenshot) {
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.waitForFunction(() => window.scrollY === 0);
    await page.screenshot({ path: resolve(screenshot), fullPage: true });
  }
  // Exercise real IPC log clearing against multiple versions and retained sites.
  const logFixtures = [
    'logs/php/old/process.log',
    'logs/postgresql/old/postgresql.log',
    'logs/sites/removed/access.log',
  ];
  for (const file of logFixtures) {
    await mkdir(dirname(join(root, 'larager-data', file)), { recursive: true });
    await writeFile(join(root, 'larager-data', file), 'old log contents\n');
  }
  await writeFile(join(root, 'larager-data/projects/protected.log'), 'project data');
  await invoke('clear_logs', { id: 'php' });
  assert.equal(await readFile(join(root, 'larager-data', logFixtures[0]), 'utf8'), '');
  assert.equal(
    await readFile(join(root, 'larager-data', logFixtures[1]), 'utf8'),
    'old log contents\n',
  );
  if (!smokeOnly) {
    await invoke('service_action', { id: 'mailpit', action: 'start' });
    const before = (await invoke('get_snapshot')).services.find(
      (service) => service.id === 'mailpit',
    );
    await invoke('clear_logs', { id: 'mailpit' });
    const after = (await invoke('get_snapshot')).services.find(
      (service) => service.id === 'mailpit',
    );
    assert.equal(after.pid, before.pid);
    assert.equal(after.status, 'running');
    await invoke('service_action', { id: 'mailpit', action: 'stop' });
  }
  await invoke('clear_logs', { id: null });
  for (const file of logFixtures)
    assert.equal(await readFile(join(root, 'larager-data', file), 'utf8'), '');
  assert.equal(
    await readFile(join(root, 'larager-data/projects/protected.log'), 'utf8'),
    'project data',
  );
  const stateBeforeReset = await invoke('get_snapshot');
  const configuration = join(root, 'larager-data/config/custom');
  await mkdir(configuration, { recursive: true });
  await writeFile(join(configuration, 'old.ini'), 'custom settings before reset');
  const preserved = ['projects/kept.txt', 'data/kept.bin', 'logs/kept.log', 'secrets/kept.bin'];
  for (const file of preserved)
    await writeFile(join(root, 'larager-data', file), `preserved ${file}`);
  if (!smokeOnly) await invoke('service_action', { id: 'mailpit', action: 'start' });
  await invoke('save_close_behavior', { behavior: 'exit' });
  await page.locator('nav').getByRole('button', { name: 'Settings', exact: true }).click();
  await page.locator('.configuration-reset-panel .button').click();
  const resetDialog = page.locator('.configuration-reset-dialog');
  await expect(resetDialog).toBeVisible();
  const reset = resetDialog.getByRole('button', { name: 'Reset all settings', exact: true });
  await expect(reset).toBeDisabled();
  await resetDialog
    .getByText('I understand that custom settings and registered domains will be removed.', {
      exact: true,
    })
    .click();
  await reset.click();
  await expect(resetDialog).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('.configuration-reset-result')).toBeVisible();
  const backup = await page.locator('.configuration-reset-result code').textContent();
  assert.ok(backup);
  assert.equal(
    JSON.parse(await readFile(join(backup, 'settings.json'), 'utf8')).closeBehavior,
    'exit',
  );
  assert.equal(
    await readFile(join(backup, 'config/custom/old.ini'), 'utf8'),
    'custom settings before reset',
  );
  await assert.rejects(readFile(join(configuration, 'old.ini')));
  for (const file of preserved)
    assert.equal(await readFile(join(root, 'larager-data', file), 'utf8'), `preserved ${file}`);
  const resetState = await invoke('get_snapshot');
  assert.equal(resetState.settings.language, 'auto');
  assert.equal(resetState.settings.theme, 'system');
  assert.equal(resetState.settings.closeBehavior, 'ask');
  assert.equal(
    resetState.services.some((service) => service.pid !== null),
    false,
  );
  for (const id of Object.keys(stateBeforeReset.settings.services)) {
    assert.deepEqual(
      resetState.settings.services[id].installations,
      stateBeforeReset.settings.services[id].installations,
    );
    assert.equal(
      resetState.settings.services[id].activeVersion,
      stateBeforeReset.settings.services[id].activeVersion,
    );
  }
  await invoke('save_preferences', { language: 'en', theme: 'light' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  const powershell = join(
    process.env.SystemRoot ?? 'C:\\Windows',
    'System32/WindowsPowerShell/v1.0/powershell.exe',
  );
  const shell = (script) =>
    promisify(execFile)(powershell, ['-NoProfile', '-NonInteractive', '-Command', script], {
      windowsHide: true,
    });
  const handle = Number(
    (await shell(`(Get-Process -Id ${child.pid}).MainWindowHandle`)).stdout.trim(),
  );
  assert.ok(Number.isSafeInteger(handle) && handle > 0, 'Native main window handle is missing');
  // tray-icon 0.25 uses tray_icon_app / callback 6002 with WM_LBUTTONUP (514).
  // Restore through this process's real tray handler. External ShowWindow calls
  // desynchronize Tao's cached visibility flags and invalidate subsequent checks.
  const windowHelper = `using System;
using System.Runtime.InteropServices;
public class TrayCheckWindow {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string name, string title);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint process);
  public static void Restore(uint process) {
    IntPtr window = IntPtr.Zero;
    while ((window = FindWindowEx(IntPtr.Zero, window, "tray_icon_app", null)) != IntPtr.Zero) {
      uint owner;
      GetWindowThreadProcessId(window, out owner);
      if (owner == process) {
        if (!PostMessage(window, 6002, IntPtr.Zero, new IntPtr(514))) throw new Exception("Tray callback failed");
        return;
      }
    }
    throw new Exception("Tray window was not registered");
  }
}`;
  const nativeWindow = (action) =>
    shell(
      `Add-Type -TypeDefinition '${windowHelper}'; ${
        action === 'minimize'
          ? `[TrayCheckWindow]::PostMessage([IntPtr]${handle}, 274, [IntPtr]61472, [IntPtr]::Zero)`
          : action === 'close'
            ? `[TrayCheckWindow]::PostMessage([IntPtr]${handle}, 16, [IntPtr]::Zero, [IntPtr]::Zero)`
            : `[TrayCheckWindow]::Restore(${child.pid})`
      } | Out-Null`,
    );
  const visible = () => invoke('plugin:window|is_visible', { label: 'main' });
  const minimized = () => invoke('plugin:window|is_minimized', { label: 'main' });
  const reopenExecutable = async () => {
    const before = await invoke('get_snapshot');
    const second = spawn(executable, [], {
      cwd: temporaryRoot, // Activation must not depend on the launch directory.
      windowsHide: true,
      stdio: ['ignore', 'inherit', 'inherit'],
    });
    try {
      const code = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          second.kill();
          reject(new Error('Second launch did not hand off to the existing app'));
        }, 15_000);
        second.once('error', (error) => {
          clearTimeout(timer);
          reject(error);
        });
        second.once('exit', (code) => {
          clearTimeout(timer);
          resolve(code);
        });
      });
      assert.equal(code, 0);
      await expect.poll(visible).toBe(true);
      await expect.poll(minimized).toBe(false);
      assert.equal(exited, false, 'Reopening replaced or terminated the first process');
      const after = await invoke('get_snapshot');
      assert.equal(after.settings.revision, before.settings.revision);
      assert.deepEqual(
        after.services.map((service) => service.pid),
        before.services.map((service) => service.pid),
      );
    } finally {
      if (second.exitCode === null) second.kill();
    }
  };
  const restoreWindow = async () => {
    await nativeWindow('restore');
    await expect.poll(visible).toBe(true);
  };
  if (!smokeOnly) {
    const ports = [await freePort(), await freePort()];
    while (ports[0] === ports[1]) ports[1] = await freePort();
    const service = (await invoke('get_snapshot')).settings.services.mailpit;
    await invoke('save_service', {
      id: 'mailpit',
      version: service.activeVersion,
      ports,
      autoStart: false,
    });
    await invoke('service_action', { id: 'mailpit', action: 'start' });
  }
  const dialog = page.getByRole('dialog', { name: 'What would you like to do?', exact: true });
  assert.equal(initial.settings.closeBehavior, 'ask');
  await nativeWindow('close');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Remember my choice', { exact: true }).click();
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(dialog).toBeHidden();
  assert.equal((await invoke('get_snapshot')).settings.closeBehavior, 'ask');
  for (const remember of [false, true]) {
    await nativeWindow('close');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('checkbox')).not.toBeChecked();
    if (remember) await dialog.getByText('Remember my choice', { exact: true }).click();
    await dialog.getByRole('button', { name: 'Minimize to tray', exact: true }).click();
    await expect.poll(visible).toBe(false);
    assert.equal(exited, false, 'Minimizing to the tray terminated the process');
    assert.equal((await invoke('get_snapshot')).settings.closeBehavior, remember ? 'tray' : 'ask');
    await reopenExecutable();
  }
  await nativeWindow('close');
  await expect.poll(visible).toBe(false);
  assert.equal(await invoke('get_close_request'), false);
  await restoreWindow();
  await nativeWindow('minimize');
  await expect.poll(minimized).toBe(true);
  await reopenExecutable();
  await invoke('save_close_behavior', { behavior: 'ask' });
  await nativeWindow('close');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Remember my choice', { exact: true }).click();
  assert.deepEqual(errors, []);
  await dialog.getByRole('button', { name: 'Exit application', exact: true }).click();
  for (let attempt = 0; attempt < 60 && !exited; attempt++) await delay(250);
  assert.equal(exited, true, 'The main window did not shut down cleanly');
  assert.equal(exitCode, 0, 'The application exited with an error');
  const stored = JSON.parse(await readFile(join(root, 'larager-data/settings.json'), 'utf8'));
  assert.equal(stored.closeBehavior, 'exit');
  console.log(
    smokeOnly
      ? 'Verified packaged WebView2, real IPC, portable storage, separate browser profile, scoped/all log clearing, configuration reset/backup/data preservation, close prompt/cancel, tray hiding, executable reactivation from hidden/minimized states, remembered preferences, and clean exit.'
      : 'Verified packaged WebView2, real IPC, empty first run, portable storage, separate browser profile, Mailpit download/start/stop, log clearing with an active process, local Monaco, configuration reset/backup/data preservation, close prompt/cancel, tray hiding, executable reactivation from hidden/minimized states, remembered preferences, and clean exit.',
  );
} finally {
  if (!exited) {
    child.kill();
    await delay(1000);
  }
  await browser?.close().catch(() => {});
  await rm(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
}
