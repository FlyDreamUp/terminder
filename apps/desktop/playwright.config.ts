import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: '../../tests/e2e', timeout: 30000, fullyParallel: false,
  use: { channel: process.platform === 'win32' ? 'msedge' : undefined, baseURL: 'http://127.0.0.1:1420', viewport: { width: 1100, height: 720 }, headless: true },
  webServer: { command: 'npm run dev', url: 'http://127.0.0.1:1420', reuseExistingServer: !process.env.CI },
});
