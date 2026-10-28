import { expect, test, type Page } from '@playwright/test';
import { serviceIds } from '../src/types';
import { defaultPorts, services } from '../src/serviceRegistry';
import catalog from '../src-tauri/catalog/packages.json' with { type: 'json' };

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
    ({ ids, ports, definitions }) => {
      let rules: string | null = null;
      const defaults =
        'index index.php index.html;\nclient_max_body_size 64m;\nlocation / { try_files $uri $uri/ /index.php?$query_string; }\n';
      const siteConfig = (content: string | null) => ({
        content: content ?? defaults,
        generated: `server {\n    listen 127.0.0.1:8081;\n    server_name my-site.test;\n    root C:/Projects/example/public;\n    location ~ \\.php$ { fastcgi_pass 127.0.0.1:9000; }\n${content ?? defaults}\n}`,
        path: 'C:\\Larager\\config\\sites\\test-domain\\nginx.conf',
        customized: content !== null,
      });
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
                addToPath: false,
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
            if (command === 'get_snapshot') {
              for (const status of state.services) {
                const definition = definitions[status.id];
                if (status.version && !definition.daemon) {
                  status.status = definition.dependencies.some(
                    (dependency) => !state.settings.services[dependency].activeVersion,
                  )
                    ? 'dependencyMissing'
                    : 'ready';
                }
              }
              return structuredClone(state);
            }
            if (command.startsWith('plugin:event|')) return 1;
            if (command === 'install_package' && status) {
              const version = String(args.version);
              service.installations.push({
                version,
                installedAt: Date.now(),
                sha256: 'a'.repeat(64),
              });
              if (!service.activeVersion) {
                service.activeVersion = version;
                status.version = version;
                status.status = definitions[id as keyof typeof definitions].daemon
                  ? 'stopped'
                  : 'ready';
              }
              if (args.addToPath) service.addToPath = true;
            } else if (command === 'service_action' && status) {
              if (!definitions[id as keyof typeof definitions].daemon)
                throw new Error('Terminal tools have no managed process');
              status.status = args.action === 'stop' ? 'stopped' : 'running';
              status.pid = status.status === 'running' ? 1234 : null;
            } else if (command === 'save_service') {
              service.activeVersion = args.version as string;
              service.ports = args.ports as number[];
              service.autoStart = Boolean(args.autoStart);
              if (status) status.version = service.activeVersion;
            } else if (command === 'get_path_status') {
              const directory = service.activeVersion
                ? `C:\\Larager\\packages\\${id}\\${service.activeVersion}`
                : null;
              return {
                enabled: service.addToPath,
                present: service.addToPath,
                directory,
                version: service.activeVersion,
                command: id === 'php' ? 'php.exe' : `${id}.exe`,
                firstMatch: service.addToPath ? `${directory}\\${id}.exe` : null,
                shadowed: false,
                external: false,
                phpConfig:
                  id === 'php'
                    ? `C:\\Larager\\config\\php\\${service.activeVersion}\\php.ini`
                    : null,
              };
            } else if (command === 'set_user_path') {
              service.addToPath = Boolean(args.enabled);
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
            } else if (command === 'read_site_config') return siteConfig(rules);
            else if (command === 'preview_site_config')
              return siteConfig(args.content as string | null);
            else if (command === 'save_site_config') {
              if (String(args.content).includes('invalid_value'))
                throw new Error('nginx: [emerg] invalid value');
              rules = args.content as string | null;
              state.settings.sites[0].nginxDirectives = rules;
            } else if (command === 'apply_hosts') state.hostsSynced = true;
            else if (command === 'choose_folder') return 'C:\\Projects\\example\\public';
            else if (command === 'remove_site') {
              state.settings.sites = [];
              state.hostsSynced = false;
            } else if (command === 'remove_package' && status) {
              service.installations = service.installations.filter(
                (install) => install.version !== args.version,
              );
              if (service.activeVersion === args.version)
                service.activeVersion = service.installations[0]?.version ?? null;
              if (!service.activeVersion) service.addToPath = false;
              status.version = service.activeVersion;
              status.status = service.activeVersion
                ? definitions[id as keyof typeof definitions].daemon
                  ? 'stopped'
                  : 'ready'
                : 'notInstalled';
            }
            return null;
          },
        },
      });
    },
    { ids: [...serviceIds], ports: defaultPorts, definitions: services },
  );
}

test('clean start, responsive pages and complete locale switching', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await expect(
    page.getByRole('heading', { name: 'A little less setup. A lot more building.' }),
  ).toBeVisible();
  await expect(page.getByText('Not installed', { exact: true })).toHaveCount(serviceIds.length);
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

test('Node.js bundles npm and uses tool controls while PATH follows its selected version', async ({
  page,
}) => {
  await mockNative(page);
  await page.goto('/');
  await page
    .getByTestId('service-nodejs')
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  const drawer = page.getByRole('dialog', { name: 'Node.js / npm settings', exact: true });
  const nodes = catalog.packages.filter((pkg) => pkg.service === 'nodejs');
  await expect(
    drawer.getByText(`Included: npm ${nodes[0].bundledTools?.npm}`, { exact: true }),
  ).toBeVisible();
  await expect(drawer.getByRole('tab')).toHaveCount(1);
  await expect(drawer.getByRole('spinbutton')).toHaveCount(0);
  await expect(
    drawer.getByRole('checkbox', { name: 'Start when Open Larager opens', exact: true }),
  ).toHaveCount(0);
  await drawer
    .getByRole('checkbox', { name: 'Add the active version to my user PATH', exact: true })
    .focus();
  await page.keyboard.press('Space');
  await drawer.getByRole('button', { name: 'Download & install', exact: true }).click();
  await expect(drawer.getByText('Ready', { exact: true })).toBeVisible();
  await expect(drawer.getByRole('button', { name: 'Start', exact: true })).toHaveCount(0);
  const install = drawer.getByRole('combobox', { name: 'Choose a version', exact: true });
  await install.click();
  await drawer.getByRole('option', { name: nodes[1].version, exact: true }).click();
  await drawer.getByRole('button', { name: 'Download & install', exact: true }).click();
  await expect(
    drawer.getByText(`Active version for PATH: Node.js / npm ${nodes[0].version}`, { exact: true }),
  ).toBeVisible();
  await drawer.getByRole('combobox', { name: 'Active version', exact: true }).click();
  await drawer.getByRole('option', { name: nodes[1].version, exact: true }).click();
  await drawer.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(
    drawer.getByText(`Active version for PATH: Node.js / npm ${nodes[1].version}`, { exact: true }),
  ).toBeVisible();
  await drawer.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(
    page.getByTestId('service-nodejs').getByRole('button', { name: 'Manage', exact: true }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Start all', exact: true })).toBeDisabled();
  await navigate(page, 'Logs');
  await expect(
    page.getByRole('combobox', { name: 'Select a service', exact: true }),
  ).toBeDisabled();
});

test('Composer installation is independent and PATH requires the selected PHP', async ({
  page,
}) => {
  await mockNative(page);
  await page.goto('/');
  await page
    .getByTestId('service-composer')
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  const composer = page.getByRole('dialog', { name: 'Composer settings', exact: true });
  await expect(
    composer.getByRole('checkbox', { name: 'Add the active version to my user PATH', exact: true }),
  ).toBeDisabled();
  await composer.getByRole('button', { name: 'Download & install', exact: true }).click();
  await expect(composer.getByText('Dependency required', { exact: true })).toBeVisible();
  await expect(
    composer.getByRole('checkbox', { name: 'Register active Composer in user PATH', exact: true }),
  ).toBeDisabled();
  await composer.getByRole('button', { name: 'Manage PHP', exact: true }).click();
  const php = page.getByRole('dialog', { name: 'PHP settings', exact: true });
  await php.getByRole('button', { name: 'Download & install', exact: true }).click();
  await php.getByRole('button', { name: 'Close', exact: true }).click();
  await page
    .getByTestId('service-composer')
    .getByRole('button', { name: 'Manage', exact: true })
    .click();
  await expect(composer.getByText(/Composer uses the active PHP/)).toContainText(
    catalog.packages.find((pkg) => pkg.service === 'php')!.version,
  );
  await composer
    .getByRole('checkbox', { name: 'Register active Composer in user PATH', exact: true })
    .focus();
  await page.keyboard.press('Space');
  await expect(
    composer.getByRole('checkbox', { name: 'Register active Composer in user PATH', exact: true }),
  ).toBeChecked();
  await expect(composer.getByRole('tab')).toHaveCount(1);
  await expect(composer.getByRole('button', { name: 'Start', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await composer.screenshot({ path: test.info().outputPath('composer-tool.png') });
  await composer.getByRole('button', { name: 'Close', exact: true }).click();
  await page
    .getByTestId('service-php')
    .getByRole('button', { name: 'Manage PHP', exact: true })
    .click();
  const version = catalog.packages.find((pkg) => pkg.service === 'php')!.version;
  await php.getByRole('button', { name: `Remove ${version}`, exact: true }).click();
  const removal = page.getByRole('dialog', { name: 'Remove package', exact: true });
  await expect(removal.getByRole('alert')).toContainText(
    'Disable the user PATH registration for Composer',
  );
  await expect(removal.getByRole('button', { name: 'Remove package', exact: true })).toBeDisabled();
});

test('domain creation and explicit hosts synchronization', async ({ page }) => {
  await mockNative(page);
  await page.goto('/');
  await navigate(page, 'Domains & HTTPS');
  await page.getByRole('button', { name: 'Add domain', exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: 'Add domain', exact: true });
  await dialog.getByLabel('Hostname', { exact: true }).fill('my-site.test');
  await expect(dialog.getByRole('radio', { name: 'Nginx + HTTPS', exact: true })).toBeChecked();
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

test('optional user PATH follows the active version and survives additional installations', async ({
  page,
}) => {
  await mockNative(page);
  await page.goto('/');
  await page
    .getByTestId('service-php')
    .getByRole('button', { name: 'Install', exact: true })
    .click();
  const drawer = page.getByRole('dialog', { name: 'PHP settings', exact: true });
  const install = drawer.getByRole('combobox', { name: 'Choose a version', exact: true });
  const firstVersion = (await install.innerText()).trim();
  const installPath = drawer.getByRole('checkbox', {
    name: 'Add the active version to my user PATH',
    exact: true,
  });
  await expect(installPath).not.toBeChecked();
  await installPath.focus();
  await page.keyboard.press('Space');
  await drawer.getByRole('button', { name: 'Download & install', exact: true }).click();
  const path = drawer.getByRole('checkbox', {
    name: 'Register active PHP in user PATH',
    exact: true,
  });
  await expect(path).toBeChecked();
  const active = drawer.getByRole('combobox', { name: 'Active version', exact: true });
  await expect(active).toHaveText(firstVersion);
  await expect(drawer.locator('.path-section')).toContainText(firstVersion);
  await install.click();
  const next = drawer.getByRole('option').nth(1);
  const secondVersion = (await next.innerText()).trim();
  await next.click();
  await expect(drawer.locator('.path-install-option')).toContainText(
    `PATH will use PHP ${firstVersion}`,
  );
  await drawer.getByRole('button', { name: 'Download & install', exact: true }).click();
  await expect(active).toHaveText(firstVersion);
  await active.click();
  await drawer.getByRole('option', { name: secondVersion, exact: true }).click();
  await drawer.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(drawer.locator('.path-section')).toContainText(secondVersion);
  await expect(drawer.locator('.path-section .path-directory').first()).toContainText(
    secondVersion,
  );
  await path.focus();
  await page.keyboard.press('Space');
  await expect(path).not.toBeChecked();
  await path.focus();
  await page.keyboard.press('Space');
  await expect(path).toBeChecked();
  await drawer.getByRole('button', { name: `Remove ${secondVersion}`, exact: true }).click();
  const removal = page.getByRole('dialog', { name: 'Remove package', exact: true });
  await expect(removal).toContainText(
    `PATH will follow the remaining active version: PHP ${firstVersion}`,
  );
  await removal.getByRole('button', { name: 'Remove package', exact: true }).click();
  await expect(active).toHaveText(firstVersion);
  await expect(path).toBeChecked();
  await drawer.getByRole('button', { name: `Remove ${firstVersion}`, exact: true }).click();
  await expect(removal).toContainText('The app’s PATH entry will also be removed');
  await removal.getByRole('button', { name: 'Remove package', exact: true }).click();
  await expect(drawer.locator('.path-section')).toHaveCount(0);
  await expect(installPath).not.toBeChecked();
});

test('per-site Nginx configuration supports preview, validation errors, persistence and reset', async ({
  page,
}) => {
  await mockNative(page);
  await page.goto('/');
  await navigate(page, 'Domains & HTTPS');
  await page.getByRole('button', { name: 'Add domain', exact: true }).first().click();
  const form = page.getByRole('dialog', { name: 'Add domain', exact: true });
  await form.getByLabel('Hostname', { exact: true }).fill('my-site.test');
  await form.getByRole('button', { name: 'Browse', exact: true }).click();
  await form.getByRole('button', { name: 'Save domain', exact: true }).click();
  await page.getByRole('button', { name: 'Site configuration my-site.test', exact: true }).click();
  const editor = page.getByRole('dialog', { name: 'Site configuration my-site.test', exact: true });
  await expect(editor.locator('.monaco-editor')).toBeVisible();
  await expect(editor.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  await expect(
    editor.getByText(
      'Install Nginx to validate and save edited rules. The base configuration is already generated.',
    ),
  ).toBeVisible();
  await editor.getByRole('button', { name: 'Preview full configuration', exact: true }).click();
  await expect(editor.locator('.monaco-editor')).toContainText('server_name my-site.test');
  await editor.getByRole('button', { name: 'Close', exact: true }).click();
  await page.getByRole('button', { name: 'Manage Nginx', exact: true }).click();
  const service = page.getByRole('dialog', { name: 'Nginx settings', exact: true });
  await service.getByRole('button', { name: 'Download & install', exact: true }).click();
  await expect(service.getByText('Stopped', { exact: true })).toBeVisible();
  await service.getByRole('button', { name: 'Close', exact: true }).click();
  await page.getByRole('button', { name: 'Site configuration my-site.test', exact: true }).click();
  const input = editor.getByRole('textbox', { name: 'Editor content', exact: true });
  await expect(input).toBeAttached();
  await input.focus();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText('client_max_body_size invalid_value;');
  await editor.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(
    page.getByRole('alert').filter({ hasText: 'nginx: [emerg] invalid value' }),
  ).toBeVisible();
  await expect(editor).toBeVisible();
  await input.focus();
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText(
    'client_max_body_size 128m;\nlocation / { try_files $uri /index.php?$query_string; }',
  );
  await editor.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(editor.getByText('Custom rules', { exact: true })).toBeVisible();
  await expect(editor.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  await editor.getByRole('button', { name: 'Close', exact: true }).click();
  await page.getByRole('button', { name: 'Site configuration my-site.test', exact: true }).click();
  await expect(editor.locator('.monaco-editor')).toContainText('128m');
  await input.focus();
  await page.keyboard.press('Control+End');
  await page.keyboard.insertText('\n# unsaved');
  await editor.getByRole('button', { name: 'Close', exact: true }).click();
  const discard = page.getByRole('dialog', {
    name: 'You have unsaved configuration changes.',
    exact: true,
  });
  await expect(discard).toBeVisible();
  await discard.getByRole('button', { name: 'Cancel', exact: true }).click();
  await editor.getByRole('button', { name: 'Restore defaults', exact: true }).click();
  await editor.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(editor.getByText('Default rules', { exact: true })).toBeVisible();
  await expect(editor.locator('.monaco-editor')).toContainText('64m');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await editor.getByRole('button', { name: 'Close', exact: true }).click();
  await page.getByRole('button', { name: 'Manage Nginx', exact: true }).click();
  await service.getByRole('button', { name: 'Start', exact: true }).click();
  await service.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(
    page.getByRole('switch', { name: 'Enabled my-site.test', exact: true }),
  ).toBeDisabled();
  await page.getByRole('button', { name: 'Site configuration my-site.test', exact: true }).click();
  await expect(
    editor.getByRole('button', { name: 'Restore defaults', exact: true }),
  ).toBeDisabled();
  await expect(editor.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  await editor.screenshot({ path: test.info().outputPath('site-configuration.png') });
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
