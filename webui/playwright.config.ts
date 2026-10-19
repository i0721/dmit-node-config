import { defineConfig, devices } from '@playwright/test';
export default defineConfig({
  testDir: './apps/web/e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 90000,
  expect: { timeout: 15000 },
  use: {
    ...devices['Desktop Chrome'],
    baseURL: process.env.NODECTL_ADMIN_TEST_URL ?? 'https://node.localhost:10443',
    ignoreHTTPSErrors: true,
    locale: 'zh-CN',
    timezoneId: 'Asia/Shanghai',
    storageState: { cookies: [], origins: [] },
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    launchOptions: {
      args: ['--no-proxy-server', '--host-resolver-rules=MAP node.localhost 127.0.0.1'],
    },
  },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  outputDir: 'test-results/playwright',
  reporter: [['list'], ['json', { outputFile: 'test-results/admin-browser.json' }]],
});
