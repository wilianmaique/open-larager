import { expect, test, type Page } from '@playwright/test';
import { serviceIds } from '../src/types';
import { defaultPorts } from '../src/serviceRegistry';

async function navigate(page: Page, name: string) {
  const menu = page.getByRole('button', { name: 'Environment', exact: true });
  if (await menu.isVisible()) await menu.click();
  await page.locator('nav').getByRole('button', { name, exact: true }).click();
  const headings: Record<string, string> = {
    Services: 'Your services',
    'Domains & HTTPS': 'Domains & HTTPS',
    Logs: 'Logs',
    Settings: 'Make it feel like home.',
  };
  await expect(page.locator('main h1')).toHaveText(headings[name]);
}

async function mockNative(page: Page) {
  await page.addInitScript(
    ({ ids, ports }) => {
      const state = {
        root: 'C:\\Users\\developer\\AppData\\Local\\dev.openlarager.desktop',
        systemLocale: 'en-US',
        hostsSynced: true,
        busyService: null,
        certificate: { available: false, trusted: false, fingerprint: null },
        settings: {
          schemaVersion: 1,
          language: 'en',
          theme: 'light',
          sites: [] as Record<string, unknown>[],
          services: Object.fromEntries(
            ids.map((id) => [
              id,
              {
                activeVersion: null as string | null,
                installations: [] as { version: string; installedAt: number; sha256: string }[],
                ports: ports[id],
                autoStart: false,
              },
            ]),
          ),
        },
        services: ids.map((id) => ({
          id,
          status: 'notInstalled',
          version: null as string | null,
          pid: null as number | null,
          uptime: 0,
          message: null,
        })),
      };
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          unregisterCallback: () => {},
          invoke: async (command: string, args: Record<string, unknown>) => {
            const id = args?.id as string;
            const service = state.settings.services[id];
            const status = state.services.find((s) => s.id === id);
            if (command === 'get_snapshot') return structuredClone(state);
            if (command.startsWith('plugin:event|')) return 1;
            if (command === 'install_package' && status) {
              const version = String(args.version);
              service.installations.push({
                version,
                installedAt: Date.now(),
                sha256: 'a'.repeat(64),
              });
              service.activeVersion = version;
              status.version = version;
              status.status = 'stopped';
            } else if (command === 'service_action' && status) {
              status.status = args.action === 'stop' ? 'stopped' : 'running';
              status.pid = status.status === 'running' ? 1234 : null;
            } else if (command === 'save_service') {
              service.activeVersion = args.version as string;
              service.ports = args.ports as number[];
              service.autoStart = Boolean(args.autoStart);
            } else if (command === 'save_preferences') {
              state.settings.language = String(args.language);
              state.settings.theme = String(args.theme);
            } else if (command === 'read_config') return 'memory_limit=256M\ndisplay_errors=On\n';
            else if (command === 'read_logs')
              return 'Service started successfully\nListening on 127.0.0.1\n';
            else if (command === 'save_site') {
              const site = args.site as Record<string, unknown>;
              state.settings.sites = [{ ...site, id: 'test-domain' }];
              state.hostsSynced = false;
            } else if (command === 'apply_hosts') state.hostsSynced = true;
            else if (command === 'choose_folder') return 'C:\\Projects\\example\\public';
            else if (command === 'remove_site') {
              state.settings.sites = [];
              state.hostsSynced = false;
            } else if (command === 'remove_package' && status) {
              service.installations = [];
              service.activeVersion = null;
              status.version = null;
              status.status = 'notInstalled';
            }
            return null;
          },
        },
      });
    },
    { ids: [...serviceIds], ports: defaultPorts },
  );
}

test('clean start, responsive pages and complete locale switching', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(
    page.getByRole('heading', { name: 'A little less setup. A lot more building.' }),
  ).toBeVisible();
  await expect(page.getByText('Not installed', { exact: true })).toHaveCount(8);
  for (const route of ['Services', 'Domains & HTTPS', 'Logs', 'Settings']) {
    await navigate(page, route);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true);
  }
  for (const [locale, heading] of [
    ['pt-BR', 'Do seu jeito.'],
    ['es', 'A tu manera.'],
    ['zh-CN', '打造顺手的环境。'],
    ['hi', 'अपने तरीके से बनाएँ।'],
    ['en', 'Make it feel like home.'],
  ]) {
    await page.locator('.language-select').selectOption(locale);
    await expect(page.getByRole('heading', { name: heading, exact: true })).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('lang', locale);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true);
  }
  await page.getByRole('button', { name: 'Dark', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(errors).toEqual([]);
});

test('version installation, service lifecycle and local Monaco editor', async ({ page }) => {
  await mockNative(page);
  const external: string[] = [];
  page.on('request', (req) => {
    if (
      !req.url().startsWith('http://127.0.0.1:1420') &&
      !req.url().startsWith('blob:') &&
      !req.url().startsWith('data:')
    )
      external.push(req.url());
  });
  await page.goto('/');
  await page
    .getByTestId('service-php')
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  const drawer = page.getByRole('dialog', { name: 'PHP settings', exact: true });
  await drawer.getByRole('button', { name: 'Download & install' }).click();
  await expect(drawer.getByText('Stopped', { exact: true })).toBeVisible();
  await drawer.getByLabel('Primary port').fill('9099');
  await drawer.getByRole('button', { name: 'Save changes', exact: true }).click();
  await drawer.getByRole('button', { name: 'Start', exact: true }).click();
  await expect(drawer.getByText('Running', { exact: true })).toBeVisible();
  await drawer.getByRole('button', { name: 'Restart', exact: true }).click();
  await drawer.getByRole('button', { name: 'Stop', exact: true }).click();
  await drawer.getByRole('tab', { name: 'Configuration', exact: true }).click();
  await expect(drawer.locator('.monaco-editor')).toBeVisible();
  expect(external).toEqual([]);
  await drawer.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.getByTestId('service-php').getByText('9099', { exact: true })).toBeAttached();
});

test('domain creation and explicit hosts synchronization', async ({ page }) => {
  await mockNative(page);
  await page.goto('/');
  await navigate(page, 'Domains & HTTPS');
  await page.getByRole('button', { name: 'Add domain', exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: 'Add domain', exact: true });
  await dialog.getByLabel('Hostname', { exact: true }).fill('my-site.test');
  await dialog.getByRole('button', { name: 'Browse', exact: true }).click();
  await expect(dialog.getByLabel('Project folder', { exact: true })).toHaveValue(
    'C:\\Projects\\example\\public',
  );
  await dialog.getByRole('button', { name: 'Save domain', exact: true }).click();
  await expect(page.locator('.domain-row')).toContainText('my-site.test');
  await expect(page.getByText('Hosts need to be applied', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Apply hosts', exact: true }).click();
  await expect(page.getByText('Hosts are in sync', { exact: true })).toBeVisible();
});

test('keyboard search opens the selected service', async ({ page }) => {
  await page.goto('/');
  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog', { name: 'Search your workspace…' });
  await dialog.getByRole('textbox').fill('postgres');
  await dialog.getByRole('button', { name: 'PostgreSQL', exact: true }).click();
  await expect(
    page.getByRole('dialog', { name: 'PostgreSQL settings', exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});
