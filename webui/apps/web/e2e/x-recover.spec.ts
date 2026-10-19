import { test, expect } from '@playwright/test';
import { login, navigate, fixtureCommand } from './fixture';
test('CLI 卸载中断可由实际 Web 恢复入口识别并恢复', async ({ page }) => {
  test.setTimeout(120000);
  const paths = [
    '/etc/dmit-node/node.toml',
    '/etc/dmit-node/secrets.json',
    '/etc/nginx/nginx.conf',
    '/etc/xray/config.json',
    '/var/lib/dmit-node/admin/credential.json',
  ];
  const before = fixtureCommand(['sha256sum', ...paths]);
  await login(page);
  await navigate(page, '运维中心');
  fixtureCommand([
    'env',
    'NODECTL_ADMIN_TEST=isolated',
    'bash',
    '/work/tests/integration/admin-interrupt-uninstall.sh',
  ]);
  await page.getByRole('button', { name: '刷新', exact: true }).click();
  const recover = page.getByRole('button', { name: '恢复中断事务', exact: true });
  await expect(recover).toBeEnabled();
  await recover.click();
  const dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: '确认执行', exact: true }).click();
  await expect(dialog).toHaveCount(0, { timeout: 90000 });
  await expect(recover).toBeDisabled();
  fixtureCommand(['test', '!', '-e', '/var/lib/dmit-node/uninstall.json']);
  fixtureCommand(['test', '!', '-e', '/var/lib/dmit-node/transaction.json']);
  expect(fixtureCommand(['sha256sum', ...paths])).toBe(before);
  expect(
    fixtureCommand([
      'systemctl',
      'is-active',
      'nginx',
      'xray',
      'nodectl-admin',
      'nodectl-admin-nginx',
    ])
      .trim()
      .split('\n'),
  ).toEqual(['active', 'active', 'active', 'active']);
});
