import { test, expect } from '@playwright/test';
import { z } from 'zod';
import {
  configurationViewSchema,
  planViewSchema,
  jobSchema,
  operationsViewSchema,
} from '@community-go/surface/node-management';
import { login, administratorToken, fixtureCommand, navigate } from './fixture';

test('Rust 强制保护、高级差异检查应用、备份恢复与回滚保持真实节点兼容', async ({ page }) => {
  test.setTimeout(300000);
  await login(page);
  await navigate(page, '配置中心');
  let csrf = '';
  const send = async (path: string, data?: unknown) =>
    page.evaluate(
      async ({ path, data, csrf }) => {
        const r = await fetch('/admin/api/' + path, {
          method: data === undefined ? 'GET' : 'POST',
          credentials: 'same-origin',
          headers:
            data === undefined ? {} : { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
          ...(data === undefined ? {} : { body: JSON.stringify(data) }),
        });
        return { status: r.status, value: (await r.json()) as unknown };
      },
      { path, data, csrf },
    );
  const session = z.object({ csrf: z.string() }).parse((await send('session')).value);
  csrf = session.csrf;
  const get = async (path: string) => {
    const r = await send(path);
    expect(r.status).toBe(200);
    return r.value;
  };
  const post = async (path: string, data: unknown, status = 200) => {
    const r = await send(path, data);
    expect(r.status).toBe(status);
    return r.value;
  };
  const task = async (path: string, data: unknown) => {
    const started = z.object({ job: z.string() }).parse(await post(path, data));
    let job = jobSchema.parse(await get('jobs/' + started.job));
    for (let n = 0; job.finished === null && n < 150; n++) {
      await new Promise<void>((r) => setTimeout(r, 1000));
      job = jobSchema.parse(await get('jobs/' + started.job));
    }
    expect(job.finished).not.toBeNull();
    expect(job.ok).toBe(true);
    return job;
  };
  const before = configurationViewSchema.parse(await get('config'));
  const originalHash = fixtureCommand([
    'sha256sum',
    '/etc/nginx/nginx.conf',
    '/etc/xray/config.json',
    '/etc/dmit-node/secrets.json',
  ]);
  const candidate = structuredClone(before.config);
  candidate.clients.xhttp_weight -= 1;
  candidate.clients.ws_weight += 1;
  expect(
    z.object({ error: z.string() }).parse(await post('config/preview', { config: candidate }, 403))
      .error,
  ).toBe('protected_field');
  expect(configurationViewSchema.parse(await get('config')).base).toBe(before.base);
  const ticket = z
    .object({ unlock: z.string() })
    .parse(await post('unlock', { token: administratorToken() }));
  const plan = planViewSchema.parse(
    await post('config/preview', { config: candidate, unlock: ticket.unlock }),
  );
  expect(plan.advanced).toBe(true);
  expect(plan.diff.length).toBeGreaterThan(0);
  await post('config/apply', { plan: plan.plan, confirm: plan.plan }, 409);
  const prepared = await task('config/deploy-preview', {
    plan: plan.plan,
    options: {
      manual_certificates: true,
      skip_dependencies: true,
      accept_ca_terms: false,
      accept_warp_terms: false,
    },
  });
  expect(prepared.result?.check_deferred_until_dependencies).toBe(true);
  await task('config/check', { plan: plan.plan });
  await post('config/apply', { plan: plan.plan, confirm: 'not-the-plan' }, 400);
  await task('config/apply', { plan: plan.plan, confirm: plan.plan });
  expect(configurationViewSchema.parse(await get('config')).config.clients).toEqual(
    candidate.clients,
  );
  const operations = operationsViewSchema.parse(await get('operations'));
  expect(operations.audit.some((a) => a.operation === 'advanced-unlock')).toBe(true);
  const backup = operations.backups.find((b) => b.id === plan.plan);
  expect(backup?.restorable).toBe(true);
  const restore = planViewSchema.parse(
    await post('backups/preview', { backup: plan.plan, token: administratorToken() }),
  );
  expect(restore.restores_identity).toBe(true);
  await task('config/check', { plan: restore.plan });
  await task('config/apply', { plan: restore.plan, confirm: restore.plan });
  expect(configurationViewSchema.parse(await get('config')).config).toEqual(before.config);
  await task('operations', { operation: 'rollback', confirm: 'rollback' });
  expect(configurationViewSchema.parse(await get('config')).config.clients).toEqual(
    candidate.clients,
  );
  await task('operations', { operation: 'rollback', confirm: 'rollback' });
  expect(configurationViewSchema.parse(await get('config')).config).toEqual(before.config);
  expect(
    fixtureCommand([
      'sha256sum',
      '/etc/nginx/nginx.conf',
      '/etc/xray/config.json',
      '/etc/dmit-node/secrets.json',
    ]),
  ).toBe(originalHash);
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
