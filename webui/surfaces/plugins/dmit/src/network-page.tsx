'use client';
import { useState } from 'react';
import { Section } from '@community-go/surface-foundation/layout';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { useNodeManagement, type JobResult } from '../../../src/node-management';
import {
  NodePage,
  Details,
  Code,
  OperationButton,
  TaskResult,
  JobProgress,
  useNodeQuery,
  useNodeText,
} from './common';
export default function NetworkPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.network);
  const t = useNodeText();
  const [result, setResult] = useState<JobResult | null>(null);
  const d = q.data;
  return (
    <NodePage name="network" loading={q.loading} error={q.error} refresh={q.refresh}>
      <JobProgress />
      {d ? (
        <>
          <Section title={t('network.native')} contentInset>
            <Details
              label={t('network.native')}
              items={[
                [t('config.ipv4'), d.native.ipv4],
                [t('config.ipv6'), d.native.ipv6 || '—'],
                [t('network.warp'), t(d.warp.enabled ? 'common.enabled' : 'common.disabled')],
                [t('network.cn'), t(d.cn_warp?.enabled ? 'common.enabled' : 'common.disabled')],
              ]}
            />
            <div className="mt-5">
              <OperationButton
                label={t('network.diagnose')}
                risk={t('network.diagnoseRisk')}
                input={{ operation: 'network-check' }}
                onResult={setResult}
              />
            </div>
          </Section>
          <Section title={t('network.interfaces')}>
            <DataTable
              label={t('network.interfaces')}
              rows={(d.interfaces ?? []).map((i) => ({ ...i, id: i.ifname }))}
              emptyContent={t('common.unavailable')}
              columns={[
                {
                  id: 'name',
                  label: t('overview.interface'),
                  rowHeader: true,
                  render: (i) => i.ifname,
                },
                { id: 'state', label: t('network.state'), render: (i) => i.operstate ?? '—' },
                { id: 'mtu', label: t('network.mtu'), render: (i) => i.mtu ?? '—' },
                {
                  id: 'address',
                  label: t('network.address'),
                  render: (i) =>
                    i.addr_info?.map((a) => a.local + '/' + a.prefixlen).join(' · ') ?? '—',
                },
              ]}
            />
          </Section>
          <Section title={t('network.policy')} contentInset>
            {d.cn_warp ? (
              <Details
                label={t('network.cn')}
                items={[
                  [t('overview.interface'), d.cn_warp.interface],
                  ['Routing table', d.cn_warp.routing_table],
                  ['Rule priority', d.cn_warp.rule_priority],
                  ['Socket mark', d.cn_warp.socket_mark],
                ]}
              />
            ) : (
              <p className="text-sm text-ink-muted">{t('network.noCn')}</p>
            )}
            <h3 className="mb-3 mt-5 font-semibold">{t('network.routes')}</h3>
            <Code>{d.routes_v4 ?? t('common.unavailable')}</Code>
            <h3 className="mb-3 mt-5 font-semibold">{t('network.routes')} · IPv6</h3>
            <Code>{d.routes_v6 ?? t('common.unavailable')}</Code>
            <h3 className="mb-3 mt-5 font-semibold">{t('network.rules')}</h3>
            <Code>{d.rules ?? t('common.unavailable')}</Code>
            <h3 className="mb-3 mt-5 font-semibold">{t('network.rules')} · IPv6</h3>
            <Code>{d.rules_v6 ?? t('common.unavailable')}</Code>
          </Section>
        </>
      ) : null}
      <TaskResult result={result} />
    </NodePage>
  );
}
