import { test, expect } from '@playwright/test';
import { login, navigate, fixtureCommand } from './fixture';

test('替换 CLI 后仍使用运行中的可信进程，受控重启撤销会话并保留节点', async ({ page }) => {
  test.setTimeout(90000);
  const bytes = fixtureCommand([
    'sha256sum',
    '/etc/dmit-node/node.toml',
    '/etc/dmit-node/secrets.json',
    '/etc/nginx/nginx.conf',
    '/etc/xray/config.json',
    '/var/lib/dmit-node/admin/credential.json',
  ]);
  const nodePids = fixtureCommand(['systemctl', 'show', 'nginx', 'xray', '-p', 'MainPID']);
  const oldApi = fixtureCommand(['systemctl', 'show', 'nodectl-admin', '-p', 'MainPID']);
  await login(page);
  await navigate(page, '运维中心');
  fixtureCommand([
    'install',
    '-m',
    '0755',
    '/integration-target/debug/nodectl',
    '/usr/local/bin/.nodectl-admin-upgrade-test',
  ]);
  fixtureCommand([
    'mv',
    '-f',
    '/usr/local/bin/.nodectl-admin-upgrade-test',
    '/usr/local/bin/nodectl',
  ]);
  await page.getByRole('button', { name: '完整健康检查', exact: true }).click();
  let dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: '确认执行', exact: true }).click();
  await expect(dialog).toHaveCount(0, { timeout: 30000 });
  await expect(page.getByText('操作已完成', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '重启管理 API', exact: true }).click();
  dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: '确认执行', exact: true }).click();
  await expect(page.getByRole('button', { name: '登录管理后台', exact: true })).toBeVisible({
    timeout: 30000,
  });
  await expect
    .poll(() => fixtureCommand(['systemctl', 'show', 'nodectl-admin', '-p', 'MainPID']))
    .not.toBe(oldApi);
  // A second request must work after transient systemd units are collected.
  const restartedApi = fixtureCommand(['systemctl', 'show', 'nodectl-admin', '-p', 'MainPID']);
  await login(page);
  await navigate(page, '运维中心');
  await page.getByRole('button', { name: '重启管理 API', exact: true }).click();
  await page
    .getByRole('alertdialog')
    .getByRole('button', { name: '确认执行', exact: true })
    .click();
  await expect(page.getByRole('button', { name: '登录管理后台', exact: true })).toBeVisible({
    timeout: 30000,
  });
  await expect
    .poll(() => fixtureCommand(['systemctl', 'show', 'nodectl-admin', '-p', 'MainPID']))
    .not.toBe(restartedApi);
  expect(fixtureCommand(['systemctl', 'show', 'nginx', 'xray', '-p', 'MainPID'])).toBe(nodePids);
  expect(
    fixtureCommand([
      'sha256sum',
      '/etc/dmit-node/node.toml',
      '/etc/dmit-node/secrets.json',
      '/etc/nginx/nginx.conf',
      '/etc/xray/config.json',
      '/var/lib/dmit-node/admin/credential.json',
    ]),
  ).toBe(bytes);
});
