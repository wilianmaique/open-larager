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
        __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
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
  for (const [locale, label, heading] of [
    ['pt-BR', 'Português (Brasil)', 'Do seu jeito.'],
    ['es', 'Español', 'A tu manera.'],
    ['zh-CN', '简体中文', '打造顺手的环境。'],
    ['hi', 'हिन्दी', 'अपने तरीके से बनाएँ।'],
    ['en', 'English', 'Make it feel like home.'],
  ]) {
    await page.locator('.language-select').getByRole('combobox').click();
    await page.getByRole('option', { name: label, exact: true }).click();
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

test('version installation, service lifecycle and local Monaco editor', async ({
  page,
  baseURL,
}) => {
  await mockNative(page);
  const external: string[] = [];
  const editorAssets: string[] = [];
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('request', (req) => {
    if (/\/(?:assets\/(?:ConfigEditor|monaco)|src\/ConfigEditor)/.test(req.url()))
      editorAssets.push(req.url());
    if (
      !req.url().startsWith(baseURL!) &&
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
  await drawer.getByLabel('Primary port', { exact: true }).fill('9099');
  await drawer.getByRole('button', { name: 'Save changes', exact: true }).click();
  await drawer.getByRole('button', { name: 'Start', exact: true }).click();
  await expect(drawer.getByText('Running', { exact: true })).toBeVisible();
  await drawer.getByRole('button', { name: 'Restart', exact: true }).click();
  await drawer.getByRole('button', { name: 'Stop', exact: true }).click();
  expect(editorAssets).toEqual([]);
  await drawer.getByRole('tab', { name: 'Configuration', exact: true }).click();
  await expect(drawer.locator('.monaco-editor')).toBeVisible();
  expect(editorAssets.length).toBeGreaterThan(0);
  await drawer.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.getByTestId('service-php').getByText('9099', { exact: true })).toBeAttached();
  await navigate(page, 'Logs');
  const service = page.getByRole('combobox', { name: 'Select a service', exact: true });
  await expect(service).toHaveText('PHP');
  await service.click();
  await page.getByRole('option', { name: 'PHP', exact: true }).click();
  await expect(page.locator('.log-content')).toContainText('Service started successfully');
  const live = page.getByRole('checkbox', { name: 'Live refresh', exact: true });
  await expect(live).toBeChecked();
  await live.focus();
  await page.keyboard.press('Space');
  await expect(live).not.toBeChecked();
  await page.getByText('Live refresh', { exact: true }).click();
  await expect(live).toBeChecked();
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
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

test('custom selects support keyboard selection, cancellation and dismissal', async ({ page }) => {
  await page.goto('/');
  await navigate(page, 'Logs');
  await expect(page.getByRole('combobox', { name: 'Select a service' })).toBeDisabled();
  await navigate(page, 'Settings');
  const language = page.getByRole('combobox', { name: 'Language', exact: true });
  await language.focus();
  await page.keyboard.press('ArrowDown');
  const list = page.getByRole('listbox', { name: 'Language', exact: true });
  await expect(list).toBeVisible();
  await expect(language).toBeFocused();
  await page.keyboard.press('End');
  await expect(language).toHaveAttribute(
    'aria-activedescendant',
    (await page.getByRole('option', { name: 'हिन्दी', exact: true }).getAttribute('id')) as string,
  );
  await page.keyboard.press('Escape');
  await expect(list).toBeHidden();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(language).toBeFocused();
  await page.keyboard.press('Space');
  await page.keyboard.press('p');
  await page.keyboard.press('Enter');
  await expect(page.locator('html')).toHaveAttribute('lang', 'pt-BR');
  const translated = page.getByRole('combobox', { name: 'Idioma', exact: true });
  await expect(translated).toBeFocused();
  await translated.click();
  await page.keyboard.press('Home');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('listbox')).toBeHidden();
  await expect(translated).not.toBeFocused();
  await expect(page.locator('html')).toHaveAttribute('lang', 'pt-BR');
  await translated.click();
  await page.locator('main h1').click();
  await expect(translated).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('select')).toHaveCount(0);
});

test('custom version menus stay inside the viewport and dismiss before their dialog', async ({
  page,
}) => {
  await page.goto('/');
  await page
    .getByTestId('service-php')
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  const drawer = page.getByRole('dialog', { name: 'PHP settings', exact: true });
  await expect(
    drawer.getByRole('combobox', { name: 'Active version', exact: true }),
  ).toBeDisabled();
  const version = drawer.getByRole('combobox', { name: 'Choose a version', exact: true });
  await version.click();
  const menu = drawer.getByRole('listbox');
  await expect(menu).toBeVisible();
  const bounds = await menu.boundingBox();
  const viewport = page.viewportSize()!;
  expect(bounds).not.toBeNull();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(viewport.width);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height);
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect(drawer).toBeVisible();
  await expect(version).toBeFocused();
  await version.press('ArrowDown');
  const last = drawer.getByRole('option').last();
  const label = await last.innerText();
  await page.keyboard.press('End');
  await page.keyboard.press('Enter');
  await expect(version).toHaveText(label);
  await expect(menu).toBeHidden();
  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();
});

test('custom radios, checkbox, switch and numeric steppers preserve form behavior', async ({
  page,
}) => {
  await mockNative(page);
  await page.goto('/');
  await navigate(page, 'Domains & HTTPS');
  await page.getByRole('button', { name: 'Add domain', exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: 'Add domain', exact: true });
  await dialog.getByLabel('Hostname', { exact: true }).fill('proxy.test');
  await dialog.getByText('Reverse proxy', { exact: true }).click();
  await expect(dialog.getByRole('radio', { name: 'Reverse proxy', exact: true })).toBeChecked();
  const port = dialog.getByRole('spinbutton', { name: 'Local server port', exact: true });
  await dialog.getByRole('button', { name: 'Increase Local server port', exact: true }).click();
  await expect(port).toHaveValue('3001');
  await dialog.getByRole('button', { name: 'Decrease Local server port', exact: true }).click();
  await expect(port).toHaveValue('3000');
  await port.fill('65535');
  await expect(
    dialog.getByRole('button', { name: 'Increase Local server port', exact: true }),
  ).toBeDisabled();
  await port.fill('1');
  await expect(
    dialog.getByRole('button', { name: 'Decrease Local server port', exact: true }),
  ).toBeDisabled();
  await port.fill('3000');
  const enabled = dialog.getByRole('checkbox', { name: 'Enabled', exact: true });
  await enabled.focus();
  await page.keyboard.press('Space');
  await expect(enabled).not.toBeChecked();
  await dialog.getByText('Enabled', { exact: true }).click();
  await expect(enabled).toBeChecked();
  await dialog.getByRole('button', { name: 'Save domain', exact: true }).click();
  const toggle = page.getByRole('switch', { name: 'Enabled proxy.test', exact: true });
  await expect(toggle).toBeChecked();
  await toggle.focus();
  await page.keyboard.press('Space');
  await expect(toggle).not.toBeChecked();
  await page.locator('.domain-row .switch').click();
  await expect(toggle).toBeChecked();
  await page.locator('.domain-row .switch').click();
  await expect(toggle).not.toBeChecked();
  await page.getByRole('button', { name: 'Edit proxy.test', exact: true }).click();
  const editing = page.getByRole('dialog', { name: 'Edit domain', exact: true });
  await expect(editing.getByRole('checkbox', { name: 'Enabled', exact: true })).not.toBeChecked();
  const proxy = editing.getByRole('radio', { name: 'Reverse proxy', exact: true });
  await proxy.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(editing.getByRole('radio', { name: 'Static files', exact: true })).toBeChecked();
});
