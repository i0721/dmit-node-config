import type { PluginDefinition } from '@community-go/plugin-framework/plugin';
export const pluginDefinition = {
  pluginId: 'dmit',
  mount: '/dmit',
} as const satisfies PluginDefinition;
