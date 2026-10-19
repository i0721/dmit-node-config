import { test, expect, type Page } from '@playwright/test';
import { resolve } from 'node:path';
import {
  nodeConfigurationSchema,
  configurationViewSchema,
  planViewSchema,
  type ConfigPlan,
} from '@community-go/surface/node-management';
import { login, navigate, administratorToken } from './fixture';
async function checkAndApply(page: Page, plan: ConfigPlan) {
  await page.getByRole('button', { name: '运行环境检查', exact: true }).click();
  await expect(page.getByText('检查通过，可以应用', { exact: true })).toBeVisible({
    timeout: 90000,
  });
  await page.getByLabel('二次确认计划标识', { exact: true }).fill(plan.plan);
  await page.getByRole('button', { name: '安全应用', exact: true }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: '确认执行', exact: true }).click();
  await expect(dialog).toHaveCount(0, { timeout: 90000 });
  await expect(page.getByRole('heading', { name: '变更审阅', exact: true })).toHaveCount(0);
}
test('界面高级修改与快照恢复经过真实检查、两次确认和运行任务', async ({ page }) => {
  test.setTimeout(300000);
  await login(page);
  await navigate(page, '配置中心');
  await page.getByLabel('管理员 Token', { exact: true }).fill(administratorToken());
  await page.getByRole('button', { name: '重新认证并解锁', exact: true }).click();
  const editor = page.getByLabel('高级配置 JSON', { exact: true });
  await expect(editor).toBeVisible();
  const before = nodeConfigurationSchema.parse(JSON.parse(await editor.inputValue()));
  const changed = structuredClone(before);
  changed.clients.xhttp_weight -= 1;
  changed.clients.ws_weight += 1;
  await editor.fill(JSON.stringify(changed, null, 2));
  const preview = page.waitForResponse(
    (r) => r.url().endsWith('/admin/api/config/preview') && r.request().method() === 'POST',
  );
  await page.getByRole('button', { name: '预览变更', exact: true }).click();
  const plan = planViewSchema.parse(await (await preview).json());
  await expect(page.getByRole('heading', { name: '变更审阅', exact: true })).toBeVisible();
  await expect(page.getByRole('rowheader', { name: /^clients\.xhttp_weight/ })).toBeVisible();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: resolve('docs/changes/112-dmit-console/evidence/config-advanced-review-1440.png'),
    fullPage: true,
  });
  await checkAndApply(page, plan);
  const actual = configurationViewSchema.parse(
    await page.evaluate(async () => {
      const r = await fetch('/admin/api/config');
      return (await r.json()) as unknown;
    }),
  );
  expect(actual.config.clients).toEqual(changed.clients);
  await navigate(page, '运维中心');
  const snapshots = page.getByRole('grid', { name: '配置快照', exact: true });
  await snapshots
    .getByRole('row')
    .filter({ hasText: plan.plan })
    .getByRole('button', { name: '恢复此快照', exact: true })
    .click();
  const dialog = page.getByRole('alertdialog');
  await dialog.getByLabel('管理员 Token', { exact: true }).fill(administratorToken());
  const restoreResponse = page.waitForResponse((r) =>
    r.url().endsWith('/admin/api/backups/preview'),
  );
  await dialog.getByRole('button', { name: '预览变更', exact: true }).click();
  const restore = planViewSchema.parse(await (await restoreResponse).json());
  await expect(dialog).toHaveCount(0);
  await checkAndApply(page, restore);
  expect(
    configurationViewSchema.parse(
      await page.evaluate(async () => {
        const r = await fetch('/admin/api/config');
        return (await r.json()) as unknown;
      }),
    ).config,
  ).toEqual(before);
});
