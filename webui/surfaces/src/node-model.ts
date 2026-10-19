import { z } from 'zod';

const toggle = z.object({ enabled: z.boolean() }).strict();
const node = z
  .object({
    domain: z.string(),
    reality_domain: z.string(),
    aliases: z.array(z.string()),
    ipv4: z.string(),
    ipv6: z.string(),
  })
  .strict();
export const nodeConfigurationSchema = z
  .object({
    schema_version: z.number().int(),
    node,
    ports: z.object({ http: z.number(), https: z.number(), reality: z.number() }).strict(),
    transport: z
      .object({
        xhttp: z.object({ path: z.string(), mode: z.string(), socket: z.string() }).strict(),
        ws: z.object({ path: z.string(), socket: z.string() }).strict(),
        reality: z.object({ path: z.string(), mode: z.string() }).strict(),
      })
      .strict(),
    tls: z
      .object({
        protocols: z.array(z.string()),
        http2: z.boolean(),
        http3: z.boolean(),
        alt_svc_max_age: z.number(),
        domain_cert_dir: z.string(),
        ip_cert_dir: z.string(),
      })
      .strict(),
    website: z
      .object({ root: z.string(), reserved_paths: z.array(z.string()).default([]) })
      .strict(),
    runtime: z
      .object({
        xray_binary: z.string(),
        nginx_binary: z.string(),
        usque_binary: z.string(),
        xray_user: z.string(),
        nginx_user: z.string(),
        log_root: z.string(),
      })
      .strict(),
    clients: z
      .object({
        xhttp_weight: z.number(),
        ws_weight: z.number(),
        reality_weight: z.number(),
        fingerprint_weights: z.record(z.string(), z.number()),
      })
      .strict(),
    warp: z
      .object({
        enabled: z.boolean(),
        config_file: z.string(),
        take_over_default_route: z.boolean(),
      })
      .strict(),
    vless: z.object({ encryption: z.boolean() }).strict().optional(),
    distribution: toggle.optional(),
    cn_warp: z
      .object({
        enabled: z.boolean(),
        interface: z.string(),
        config_file: z.string(),
        routing_table: z.number(),
        rule_priority: z.number(),
        socket_mark: z.number(),
        geodata_dir: z.string(),
      })
      .strict()
      .optional(),
    web_services: z
      .object({
        enabled: z.boolean(),
        librespeed_port: z.number(),
        openlist_port: z.number(),
        openlist_data_dir: z.string(),
        openlist_files_dir: z.string(),
        download_size_mib: z.number(),
      })
      .strict()
      .optional(),
  })
  .strict();
export type NodeConfiguration = z.infer<typeof nodeConfigurationSchema>;
export const serviceSchema = z.object({
  name: z.string(),
  enabled: z.boolean(),
  available: z.boolean(),
  state: z.record(z.string(), z.string()).nullable(),
});
export type ManagedService = z.infer<typeof serviceSchema>;
const networkCounter = z.object({
  interface: z.string(),
  rx_bytes: z.number(),
  tx_bytes: z.number(),
  rx_bytes_per_second: z.number().nullable(),
  tx_bytes_per_second: z.number().nullable(),
});
export const overviewSchema = z.object({
  version: z.string(),
  domain: z.string().nullable(),
  configured: z.boolean(),
  current: z.string().nullable(),
  integrity: z.boolean(),
  recovery_required: z.boolean(),
  services: z.array(serviceSchema),
  metrics: z.object({
    sample_unix: z.number(),
    source: z.string(),
    cpu_percent: z.number().nullable(),
    uptime_seconds: z.number().nullable(),
    memory: z
      .object({ total_bytes: z.number(), available_bytes: z.number(), used_bytes: z.number() })
      .nullable(),
    disk: z
      .object({ mount: z.string(), total_bytes: z.number(), available_bytes: z.number() })
      .nullable(),
    network: z.array(networkCounter).nullable(),
  }),
});
export type NodeOverview = z.infer<typeof overviewSchema>;
export const configurationViewSchema = z.object({
  config: nodeConfigurationSchema,
  assets: z.array(z.string()),
  base: z.string(),
  safe_mode: z.boolean(),
});
export type ConfigurationView = z.infer<typeof configurationViewSchema>;
export const nodesViewSchema = z.object({
  release: z.string(),
  domain: node,
  ports: z.object({ http: z.number(), https: z.number(), reality: z.number() }),
  distribution_enabled: z.boolean(),
  access: z.string(),
  protocols: z.array(z.object({ name: z.string(), port: z.number(), weight: z.number() })),
});
export type NodesView = z.infer<typeof nodesViewSchema>;
export const networkViewSchema = z.object({
  native: node,
  warp: nodeConfigurationSchema.shape.warp,
  cn_warp: nodeConfigurationSchema.shape.cn_warp.nullable(),
  interfaces: z
    .array(
      z.object({
        ifname: z.string(),
        operstate: z.string().optional(),
        mtu: z.number().optional(),
        addr_info: z
          .array(z.object({ local: z.string(), family: z.string(), prefixlen: z.number() }))
          .optional(),
      }),
    )
    .nullable(),
  routes_v4: z.string().nullable(),
  routes_v6: z.string().nullable(),
  rules_v6: z.string().nullable(),
  rules: z.string().nullable(),
  services: z.array(serviceSchema),
});
export type NetworkView = z.infer<typeof networkViewSchema>;
export const dnsViewSchema = z.object({
  records: z.array(
    z.object({ name: z.string(), A: z.string(), AAAA: z.string(), proxied: z.boolean() }),
  ),
  certificates: z.array(
    z.object({ kind: z.string(), details: z.string().nullable(), valid_24h: z.boolean() }),
  ),
  credentials_configured: z.boolean(),
  zone: z.string().nullable(),
});
export type DnsView = z.infer<typeof dnsViewSchema>;
export const planViewSchema = z.object({
  restores_identity: z.boolean().default(false),
  plan: z.string(),
  release: z.string(),
  expires_in: z.number(),
  advanced: z.boolean(),
  risk: z.string(),
  diff: z.string(),
  changes: z.array(
    z.object({
      field: z.string(),
      before: z.unknown(),
      after: z.unknown(),
      protected: z.boolean(),
    }),
  ),
});
export type ConfigPlan = z.infer<typeof planViewSchema>;
export const deploymentViewSchema = z.object({
  configuration_diff: z.string(),
  required_actions: z.array(z.string()),
  dns_changes: z.array(
    z.object({ name: z.string(), kind: z.string(), before: z.unknown(), after: z.unknown() }),
  ),
  stages: z.array(z.string()),
  recovery_limits: z.array(z.string()),
  manual_dns: z.boolean(),
  manual_certificates: z.boolean(),
});
export const jobResultSchema = z.object({
  message: z.string().optional(),
  error: z.string().optional(),
  token: z.string().optional(),
  log: z.string().optional(),
  checked: z.boolean().optional(),
  plan: z.string().optional(),
  backup: z.string().optional(),
  release: z.string().optional(),
  current: z.string().nullable().optional(),
  deployment: deploymentViewSchema.or(z.boolean()).optional(),
  deployment_digest: z.string().optional(),
  check_deferred_until_dependencies: z.boolean().optional(),
  ok: z.boolean().optional(),
  checks: z
    .array(z.object({ name: z.string(), ok: z.boolean(), detail: z.string(), remedy: z.string() }))
    .optional(),
  diagnostics: z
    .array(z.object({ name: z.string(), ok: z.boolean(), trace: z.string().nullable() }))
    .optional(),
});
export type JobResult = z.infer<typeof jobResultSchema>;
export const jobSchema = z.object({
  id: z.string(),
  operation: z.string(),
  phase: z.string(),
  started: z.number(),
  finished: z.number().nullable(),
  ok: z.boolean().nullable(),
  result: jobResultSchema.nullable(),
});
export type ManagedJob = z.infer<typeof jobSchema>;
export const operationsViewSchema = z.object({
  jobs: z.array(jobSchema),
  audit: z.array(
    z.object({
      time: z.number(),
      operation: z.string(),
      outcome: z.string(),
      id: z.string().nullable(),
    }),
  ),
  current: z.string().nullable(),
  previous: z.string().nullable(),
  recovery_required: z.boolean(),
  backups: z.array(z.object({ id: z.string(), created: z.number(), restorable: z.boolean() })),
});
export type OperationsView = z.infer<typeof operationsViewSchema>;
export type OperationName =
  | 'api-restart'
  | 'doctor'
  | 'dns-check'
  | 'network-check'
  | 'renew-certificates'
  | 'backup'
  | 'rollback'
  | 'recover'
  | 'subscription-rotate'
  | 'subscription-revoke'
  | 'logs'
  | 'service-restart'
  | 'service-reload'
  | 'upgrade';
export type OperationInput = Readonly<{
  operation: OperationName;
  confirm: string;
  service?: string;
  version?: string;
}>;
export type ProvisionOptions = Readonly<{
  manual_certificates: boolean;
  acme_email?: string;
  accept_ca_terms: boolean;
  accept_warp_terms: boolean;
  skip_dependencies: boolean;
}>;
