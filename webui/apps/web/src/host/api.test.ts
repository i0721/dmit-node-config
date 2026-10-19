import { afterEach, describe, expect, it, vi } from 'vitest';
import { createAdminApi, ApiError } from './api';
import { overviewSchema } from '@community-go/surface/node-management';
function response(value: unknown, status = 200) {
  return { ok: status === 200, status, json: () => Promise.resolve(value) };
}
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});
describe('Rust API browser adapter', () => {
  it('keeps administrator and CSRF secrets out of URLs and browser storage', async () => {
    const fetch = vi
      .fn<(url: string, init?: RequestInit) => Promise<ReturnType<typeof response>>>()
      .mockResolvedValueOnce(response({ csrf: 'csrf-value' }))
      .mockResolvedValueOnce(response({ saved: true }));
    vi.stubGlobal('fetch', fetch);
    const api = createAdminApi(vi.fn(), vi.fn());
    await api.login('administrator-secret');
    await api.port.credentials('a'.repeat(32), 'cloudflare-secret');
    expect(fetch.mock.calls[0]?.[0]).toBe('/admin/api/login');
    expect(fetch.mock.calls[0]?.[1]).toMatchObject({
      method: 'POST',
      credentials: 'same-origin',
      cache: 'no-store',
      body: JSON.stringify({ token: 'administrator-secret' }),
    });
    expect(fetch.mock.calls[1]?.[1]?.headers).toMatchObject({ 'X-CSRF-Token': 'csrf-value' });
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });
  it('invalidates local session immediately on a server 401', async () => {
    const denied = vi.fn();
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(response({ error: 'unauthorized', message: '会话失效' }, 401)),
    );
    const api = createAdminApi(denied, vi.fn());
    await expect(api.session()).rejects.toBeInstanceOf(ApiError);
    expect(denied).toHaveBeenCalledOnce();
  });
  it('polls a real task contract and surfaces a failed check without treating it as applied', async () => {
    const progress = vi.fn();
    const job = {
      id: 'job',
      operation: 'config-check',
      phase: 'failed',
      started: 1,
      finished: 2,
      ok: false,
      result: { error: 'operation_failed', message: '检查失败' },
    };
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(response({ job: 'job' }))
        .mockResolvedValueOnce(response(job)),
    );
    const api = createAdminApi(vi.fn(), progress);
    await expect(api.port.check('plan')).rejects.toThrow('检查失败');
    expect(progress).toHaveBeenCalledWith(job);
  });
  it('refuses malformed metrics instead of substituting invented numbers', () => {
    expect(
      overviewSchema.safeParse({ version: '0.5.1', metrics: { cpu_percent: '72%' } }).success,
    ).toBe(false);
  });
});
