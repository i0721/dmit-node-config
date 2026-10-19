import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { login, navigate } from './fixture';
const evidence = resolve('docs/changes/112-dmit-console/evidence');
async function auditAccessibility(page: Page) {
  await expect(page.locator('[role="log"] [role="img"]')).toHaveCount(0);
  const audit = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(
    audit.violations.map((v) => ({
      id: v.id,
      nodes: v.nodes.map((n) => ({ target: n.target, html: n.html })),
    })),
  ).toEqual([]);
}
test('真实 HTTPS 下的七页、移动导航、深色模式与可访问性', async ({ page, browser }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (
      m.type() === 'error' &&
      /Content Security Policy|Refused to|Hydration failed/i.test(m.text())
    )
      errors.push(m.text());
  });
  await mkdir(evidence, { recursive: true });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/admin/dmit/');
  await expect(page.getByLabel('管理员 Token', { exact: true })).toBeVisible();
  await auditAccessibility(page);
  await page.screenshot({ path: resolve(evidence, 'login-1440.png'), fullPage: true });
  await login(page);
  const cookies = await page.context().cookies();
  expect(
    cookies.map(({ name, httpOnly, secure, sameSite, path }) => ({
      name,
      httpOnly,
      secure,
      sameSite,
      path,
    })),
  ).toEqual([
    { name: '__Host-nodectl_admin', httpOnly: true, secure: true, sameSite: 'Strict', path: '/' },
  ]);
  await expect(page.getByText('暂不可用', { exact: true })).toHaveCount(0);
  await expect
    .poll(async () => page.locator('[role="progressbar"]').count())
    .toBeGreaterThanOrEqual(3);
  await auditAccessibility(page);
  await page.screenshot({ path: resolve(evidence, 'overview-1440.png'), fullPage: true });
  const routes = [
    ['节点与订阅', 'nodes'],
    ['配置中心', 'config'],
    ['网络与 WARP', 'network'],
    ['DNS 与证书', 'dns'],
    ['服务管理', 'services'],
    ['运维中心', 'operations'],
  ] as const;
  for (const [name, file] of routes) {
    await navigate(page, name);
    await expect(page.getByRole('heading', { name: '读取失败', exact: true })).toHaveCount(0);
    await auditAccessibility(page);
    await page.screenshot({ path: resolve(evidence, file + '-1440.png'), fullPage: true });
  }
  await page.getByRole('button', { name: '深色模式', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await auditAccessibility(page);
  await page.screenshot({ path: resolve(evidence, 'operations-dark-1440.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await navigate(page, '配置中心');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await auditAccessibility(page);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await page.screenshot({ path: resolve(evidence, 'config-dark-390.png'), fullPage: true });
  await page.getByRole('button', { name: '浅色模式', exact: true }).click();
  await navigate(page, '总览');
  await expect(page.getByRole('heading', { name: '节点运行总览', exact: true })).toBeVisible();
  await page.screenshot({ path: resolve(evidence, 'overview-390.png'), fullPage: true });
  expect(errors).toEqual([]);
  expect(
    await page.evaluate(() => Object.keys(localStorage).concat(Object.keys(sessionStorage))),
  ).toEqual([]);
  await test
    .info()
    .attach('browser-version', { body: browser.version(), contentType: 'text/plain' });
});
test('客户端导出使用当前成功发布并保存真实文件', async ({ page }) => {
  await login(page);
  await navigate(page, '节点与订阅');
  await page.getByRole('button', { name: '导出 Xray JSON', exact: true }).click();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: '保存配置文件', exact: true }).click();
  expect((await download).suggestedFilename()).toBe('nodectl-xray.json');
});
