'use client';
import { useState } from 'react';
import { Section } from '@community-go/surface-foundation/layout';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { Badge } from '@community-go/ui-adapter/feedback';
import { useNodeManagement, type JobResult } from '../../../src/node-management';
import {
  NodePage,
  OperationButton,
  TaskResult,
  JobProgress,
  useNodeQuery,
  useNodeText,
} from './common';
export default function ServicesPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.services, 10000);
  const t = useNodeText();
  const [result, setResult] = useState<JobResult | null>(null);
  return (
    <NodePage name="services" loading={q.loading} error={q.error} refresh={q.refresh}>
      <JobProgress />
      <Section title={t('services.title')}>
        <DataTable
          label={t('services.title')}
          rows={(q.data ?? []).map((s) => ({ ...s, id: s.name }))}
          emptyContent={t('common.empty')}
          columns={[
            {
              id: 'name',
              label: t('services.name'),
              rowHeader: true,
              render: (s) => <strong>{s.name}</strong>,
            },
            {
              id: 'active',
              label: t('services.active'),
              render: (s) => (
                <Badge
                  tone={
                    s.state?.ActiveState === 'active'
                      ? 'success'
                      : s.enabled
                        ? 'warning'
                        : 'neutral'
                  }
                >
                  {s.state?.ActiveState ?? t('common.unavailable')}
                </Badge>
              ),
            },
            { id: 'sub', label: t('services.sub'), render: (s) => s.state?.SubState ?? '—' },
            {
              id: 'restarts',
              label: t('services.restarts'),
              render: (s) => s.state?.NRestarts ?? '—',
            },
            {
              id: 'actions',
              label: t('services.actions'),
              render: (s) =>
                s.enabled ? (
                  <div className="flex flex-wrap gap-2">
                    <OperationButton
                      label={t('services.restart')}
                      risk={t('services.restartRisk')}
                      input={{ operation: 'service-restart', service: s.name }}
                      onResult={(r) => {
                        setResult(r);
                        void q.refresh();
                      }}
                    />
                    {s.name === 'nginx' ? (
                      <OperationButton
                        label={t('services.reload')}
                        risk={t('services.reloadRisk')}
                        input={{ operation: 'service-reload', service: s.name }}
                        onResult={(r) => {
                          setResult(r);
                          void q.refresh();
                        }}
                      />
                    ) : null}
                  </div>
                ) : (
                  <span className="text-xs text-ink-muted">{t('services.notEnabled')}</span>
                ),
            },
          ]}
        />
      </Section>
      <TaskResult result={result} />
    </NodePage>
  );
}
