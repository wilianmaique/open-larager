import { defineConfig } from '@playwright/test';

const preview = process.env.PLAYWRIGHT_PREVIEW === '1';
const baseURL = `http://127.0.0.1:${preview ? 1421 : 1420}`;

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  timeout: 30_000,
  use: {
    baseURL,
    locale: 'en-US',
    colorScheme: 'light',
    reducedMotion: 'reduce',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 1000 } } },
    { name: 'mobile', use: { viewport: { width: 390, height: 844 } } },
  ],
  webServer: {
    command: preview ? 'pnpm preview --port 1421' : 'pnpm dev',
    url: baseURL,
    reuseExistingServer: !process.env.CI && !preview,
  },
});
