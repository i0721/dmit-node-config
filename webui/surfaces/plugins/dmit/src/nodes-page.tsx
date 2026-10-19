'use client';
import { useState } from 'react';
import { Section } from '@community-go/surface-foundation/layout';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { Action } from '@community-go/ui-adapter/action';
import { AlertBanner } from '@community-go/ui-adapter/feedback';
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
export default function NodesPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.nodes);
  const t = useNodeText();
  const [result, setResult] = useState<JobResult | null>(null);
  const [content, setContent] = useState('');
  const [format, setFormat] = useState<'xray' | 'mihomo'>('xray');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const exportClients = async (format: 'xray' | 'mihomo') => {
    setLoading(true);
    try {
      setContent(await api.exportClients(format));
      setFormat(format);
      setError('');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };
  return (
    <NodePage name="nodes" loading={q.loading} error={q.error} refresh={q.refresh}>
      <JobProgress />
      {q.data ? (
        <>
          <Section title={t('nodes.protocols')}>
            <DataTable
              label={t('nodes.protocols')}
              rows={q.data.protocols.map((p) => ({ ...p, id: p.name }))}
              emptyContent={t('common.empty')}
              columns={[
                { id: 'name', label: t('nodes.protocol'), rowHeader: true, render: (p) => p.name },
                { id: 'port', label: t('nodes.port'), render: (p) => p.port },
                { id: 'weight', label: t('nodes.weight'), render: (p) => p.weight + '%' },
              ]}
            />
          </Section>
          <Section title={t('nodes.subscription')} contentInset>
            <Details
              label={t('nodes.subscription')}
              items={[
                [t('nodes.domain'), q.data.domain.domain],
                [t('nodes.reality'), q.data.domain.reality_domain],
                [t('nodes.status'), q.data.access],
                [
                  t('nodes.distribution'),
                  t(q.data.distribution_enabled ? 'common.enabled' : 'common.disabled'),
                ],
              ]}
            />
            <div className="mt-5 flex flex-wrap gap-3">
              <OperationButton
                label={t('nodes.rotate')}
                risk={t('nodes.rotateRisk')}
                input={{ operation: 'subscription-rotate' }}
                disabled={!q.data.distribution_enabled}
                onResult={(r) => {
                  setResult(r);
                  void q.refresh();
                }}
              />
              <OperationButton
                label={t('nodes.revoke')}
                risk={t('nodes.revokeRisk')}
                input={{ operation: 'subscription-revoke' }}
                disabled={!q.data.distribution_enabled}
                onResult={(r) => {
                  setResult(r);
                  void q.refresh();
                }}
              />
            </div>
          </Section>
        </>
      ) : null}
      <Section title={t('nodes.export')} description={t('nodes.exportRisk')} contentInset>
        <div className="flex flex-wrap gap-3">
          <Action
            variant="secondary"
            disabled={!q.data || loading}
            loading={loading}
            onPress={() => {
              void exportClients('xray');
            }}
          >
            {t('nodes.xray')}
          </Action>
          <Action
            variant="secondary"
            disabled={!q.data || loading}
            onPress={() => {
              void exportClients('mihomo');
            }}
          >
            {t('nodes.mihomo')}
          </Action>
        </div>
        {content ? (
          <div className="mt-4 grid gap-3">
            <div>
              <Action variant="secondary" onPress={() => api.downloadClients(format, content)}>
                {t('nodes.saveFile')}
              </Action>
            </div>
            <Code>{content}</Code>
          </div>
        ) : null}
        {error ? (
          <AlertBanner icon="!" tone="danger" title={t('common.failed')} description={error} />
        ) : null}
      </Section>
      <TaskResult result={result} />
    </NodePage>
  );
}
