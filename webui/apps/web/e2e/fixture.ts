import { execFileSync } from 'node:child_process';
import { expect, type Page } from '@playwright/test';
const container = process.env.NODECTL_ADMIN_TEST_CONTAINER ?? 'nodectl-admin-e2e-01a1519d';
function docker(args: string[]) {
  return process.platform === 'win32'
    ? execFileSync('wsl.exe', ['-u', 'root', '--exec', 'docker', ...args], {
        encoding: 'utf8',
        maxBuffer: 2 * 1024 * 1024,
      })
    : execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 2 * 1024 * 1024 });
}
export function fixtureCommand(args: string[]) {
  if (
    !/^nodectl-admin-e2e-[a-z0-9-]+$/.test(container) ||
    docker([
      'inspect',
      '--format',
      '{{index .Config.Labels "nodectl.isolated-test"}}',
      container,
    ]).trim() !== 'true'
  )
    throw new Error('An explicitly labelled disposable fixture is required');
  return docker(['exec', container, ...args]);
}
export function administratorToken() {
  const token = fixtureCommand(['cat', '/root/nodectl-test/admin-token']).trim();
  if (!/^[0-9a-f]{64}$/.test(token)) throw new Error('Fixture credential unavailable');
  return token;
}
export async function login(page: Page) {
  await page.goto('/admin/dmit/');
  await page.getByLabel('管理员 Token', { exact: true }).fill(administratorToken());
  await page.getByRole('button', { name: '登录管理后台', exact: true }).click();
  await expect(page.getByRole('heading', { name: '节点运行总览', exact: true })).toBeVisible();
}
export async function navigate(page: Page, name: string) {
  if (await page.getByRole('button', { name: '导航', exact: true }).isVisible())
    await page.getByRole('button', { name: '导航', exact: true }).click();
  await page.getByRole('link', { name, exact: true }).click();
  await expect(
    page.getByRole('heading', {
      name: name === '总览' ? '节点运行总览' : name,
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
}
