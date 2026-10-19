'use client';
/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, type ReactNode } from 'react';
import type {
  ConfigurationView,
  ConfigPlan,
  DnsView,
  JobResult,
  ManagedJob,
  ManagedService,
  NetworkView,
  NodeConfiguration,
  NodeOverview,
  NodesView,
  OperationInput,
  OperationsView,
  ProvisionOptions,
} from './node-model';
export * from './node-model';
export type NodeManagementPort = Readonly<{
  overview: () => Promise<NodeOverview>;
  configuration: () => Promise<ConfigurationView>;
  nodes: () => Promise<NodesView>;
  network: () => Promise<NetworkView>;
  dns: () => Promise<DnsView>;
  services: () => Promise<readonly ManagedService[]>;
  operations: () => Promise<OperationsView>;
  unlock: (token: string) => Promise<string>;
  preview: (config: NodeConfiguration, unlock?: string) => Promise<ConfigPlan>;
  restoreBackup: (backup: string, token: string) => Promise<ConfigPlan>;
  check: (plan: string) => Promise<JobResult>;
  prepareDeployment: (plan: string, options: ProvisionOptions) => Promise<JobResult>;
  apply: (plan: string, confirm: string) => Promise<JobResult>;
  operate: (input: OperationInput) => Promise<JobResult>;
  credentials: (zone: string, token: string) => Promise<void>;
  exportClients: (format: 'xray' | 'mihomo') => Promise<string>;
  downloadClients: (format: 'xray' | 'mihomo', content: string) => void;
  job: ManagedJob | null;
}>;
const Context = createContext<NodeManagementPort | null>(null);
export function NodeManagementProvider({
  port,
  children,
}: Readonly<{ port: NodeManagementPort; children: ReactNode }>) {
  return <Context.Provider value={port}>{children}</Context.Provider>;
}
export function useNodeManagement() {
  const port = useContext(Context);
  if (!port) throw new Error('NodeManagementProvider is required');
  return port;
}
