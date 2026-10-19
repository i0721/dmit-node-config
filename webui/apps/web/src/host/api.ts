import { z } from 'zod';
import {
  overviewSchema,
  configurationViewSchema,
  nodesViewSchema,
  networkViewSchema,
  dnsViewSchema,
  operationsViewSchema,
  planViewSchema,
  jobSchema,
  jobResultSchema,
  serviceSchema,
  type NodeManagementPort,
  type ManagedJob,
  type JobResult,
} from '@community-go/surface/node-management';
const sessionSchema = z.object({ csrf: z.string() });
const taskSchema = z.object({ job: z.string() });
export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}
export function createAdminApi(onUnauthorized: () => void, onJob: (job: ManagedJob) => void) {
  let csrf = '';
  async function request<T>(path: string, schema: z.ZodType<T>, body?: unknown): Promise<T> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 180000);
    try {
      const response = await fetch('/admin/api/' + path, {
        method: body === undefined ? 'GET' : 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
        headers:
          body === undefined ? {} : { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        signal: controller.signal,
      });
      let value: unknown;
      try {
        value = await response.json();
      } catch {
        throw new ApiError(
          'invalid_response',
          response.status === 413
            ? '配置请求超过 128 KiB 限制。'
            : 'API 返回了非 JSON 响应，请检查管理服务和 HTTPS 代理。',
        );
      }
      if (!response.ok) {
        const error = z.object({ error: z.string(), message: z.string() }).safeParse(value);
        if (response.status === 401) {
          csrf = '';
          onUnauthorized();
        }
        throw new ApiError(
          error.success ? error.data.error : 'request_failed',
          error.success ? error.data.message : 'API 请求失败',
        );
      }
      return schema.parse(value);
    } finally {
      clearTimeout(timeout);
    }
  }
  async function watch(path: string, body: unknown): Promise<JobResult> {
    const task = await request(path, taskSchema, body);
    for (;;) {
      const job = await request('jobs/' + task.job, jobSchema);
      const progressResult = job.result ? { ...job.result } : null;
      if (progressResult) delete progressResult.token;
      onJob({ ...job, result: progressResult });
      if (job.finished !== null) {
        if (job.ok !== true)
          throw new ApiError(
            job.result?.error ?? 'operation_failed',
            job.result?.message ?? '操作失败，请检查事务和依赖状态。',
          );
        return job.result ?? jobResultSchema.parse({});
      }
      await new Promise<void>((resolve) => setTimeout(resolve, 1000));
    }
  }
  const port: Omit<NodeManagementPort, 'job'> = {
    restoreBackup: (backup, token) => request('backups/preview', planViewSchema, { backup, token }),
    downloadClients: (format, content) => {
      const url = URL.createObjectURL(
        new Blob([content], { type: format === 'xray' ? 'application/json' : 'text/yaml' }),
      );
      const link = document.createElement('a');
      link.href = url;
      link.download = format === 'xray' ? 'nodectl-xray.json' : 'nodectl-mihomo.yaml';
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    },
    overview: () => request('overview', overviewSchema),
    configuration: () => request('config', configurationViewSchema),
    nodes: () => request('nodes', nodesViewSchema),
    network: () => request('network', networkViewSchema),
    dns: () => request('dns', dnsViewSchema),
    services: async () =>
      (await request('services', z.object({ services: z.array(serviceSchema) }))).services,
    operations: () => request('operations', operationsViewSchema),
    unlock: async (token) =>
      (await request('unlock', z.object({ unlock: z.string() }), { token })).unlock,
    preview: (config, unlock) =>
      request('config/preview', planViewSchema, { config, ...(unlock ? { unlock } : {}) }),
    check: (plan) => watch('config/check', { plan }),
    prepareDeployment: (plan, options) => watch('config/deploy-preview', { plan, options }),
    apply: (plan, confirm) => watch('config/apply', { plan, confirm }),
    operate: (input) => watch('operations', input),
    credentials: async (zone, token) => {
      await request('credentials', z.object({ saved: z.boolean() }), { zone, token });
    },
    exportClients: async (format) =>
      (await request('clients/' + format, z.object({ content: z.string() }))).content,
  };
  return {
    port,
    session: async () => {
      const s = await request('session', sessionSchema);
      csrf = s.csrf;
    },
    login: async (token: string) => {
      const s = await request('login', sessionSchema, { token });
      csrf = s.csrf;
    },
    logout: async () => {
      await request('logout', z.object({ ok: z.boolean() }), {});
      csrf = '';
      onUnauthorized();
    },
  };
}
