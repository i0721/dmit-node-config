'use client';
import { Section } from '@community-go/surface-foundation/layout';
import { Card, CardContent, CardHeader } from '@community-go/ui-adapter/card';
import { ProgressMeter } from '@community-go/ui-adapter/progress-meter';
import { AlertBanner, Badge } from '@community-go/ui-adapter/feedback';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { useNodeManagement } from '../../../src/node-management';
import { NodePage, Details, bytes, number, Timestamp, useNodeQuery, useNodeText } from './common';
export default function OverviewPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.overview, 5000);
  const t = useNodeText();
  const d = q.data;
  const m = d?.metrics;
  return (
    <NodePage name="overview" loading={q.loading} error={q.error} refresh={q.refresh}>
      {d?.recovery_required ? (
        <AlertBanner
          icon="!"
          tone="danger"
          title={t('overview.recovery')}
          description={t('overview.recoveryDescription')}
        />
      ) : null}
      {d && !d.configured ? (
        <AlertBanner
          icon="!"
          tone="warning"
          title={t('overview.notConfigured')}
          description={t('overview.setup')}
        />
      ) : null}
      {m ? (
        <>
          <div className="grid gap-5 md:grid-cols-3">
            <Card>
              <CardHeader title={t('overview.cpu')} />
              <CardContent>
                {m.cpu_percent === null ? (
                  <p className="text-ink-muted">{t('overview.twoSamples')}</p>
                ) : (
                  <ProgressMeter
                    label={t('overview.cpu')}
                    value={Math.round(m.cpu_percent * 10) / 10}
                  />
                )}
              </CardContent>
            </Card>
            <Card>
              <CardHeader title={t('overview.memory')} />
              <CardContent>
                {m.memory ? (
                  <>
                    <p className="mb-3 text-sm text-ink-muted">
                      {bytes(m.memory.used_bytes)} / {bytes(m.memory.total_bytes)}
                    </p>
                    <ProgressMeter
                      label={t('overview.memory')}
                      value={Math.round((100 * m.memory.used_bytes) / m.memory.total_bytes)}
                    />
                  </>
                ) : (
                  t('common.unavailable')
                )}
              </CardContent>
            </Card>
            <Card>
              <CardHeader title={t('overview.disk')} />
              <CardContent>
                {m.disk ? (
                  <>
                    <p className="mb-3 text-sm text-ink-muted">
                      {bytes(m.disk.total_bytes - m.disk.available_bytes)} /{' '}
                      {bytes(m.disk.total_bytes)}
                    </p>
                    <ProgressMeter
                      label={t('overview.disk')}
                      value={Math.round(
                        (100 * (m.disk.total_bytes - m.disk.available_bytes)) / m.disk.total_bytes,
                      )}
                    />
                  </>
                ) : (
                  t('common.unavailable')
                )}
              </CardContent>
            </Card>
          </div>
          <Section title={t('overview.network')}>
            <DataTable
              label={t('overview.network')}
              rows={(m.network ?? []).map((n) => ({ ...n, id: n.interface }))}
              emptyContent={t('common.unavailable')}
              columns={[
                {
                  id: 'interface',
                  label: t('overview.interface'),
                  rowHeader: true,
                  render: (r) => r.interface,
                },
                { id: 'rx', label: t('overview.rx'), render: (r) => bytes(r.rx_bytes) },
                { id: 'tx', label: t('overview.tx'), render: (r) => bytes(r.tx_bytes) },
                {
                  id: 'rxRate',
                  label: t('overview.rxRate'),
                  render: (r) =>
                    r.rx_bytes_per_second === null ? '—' : bytes(r.rx_bytes_per_second) + '/s',
                },
                {
                  id: 'txRate',
                  label: t('overview.txRate'),
                  render: (r) =>
                    r.tx_bytes_per_second === null ? '—' : bytes(r.tx_bytes_per_second) + '/s',
                },
              ]}
            />
            <p className="px-5 py-3 text-xs text-ink-muted">
              {t('overview.source')}: {m.source} · <Timestamp value={m.sample_unix} />
            </p>
          </Section>
        </>
      ) : null}
      {d ? (
        <>
          <Section title={t('overview.services')}>
            <DataTable
              label={t('overview.services')}
              rows={d.services.map((s) => ({ ...s, id: s.name }))}
              emptyContent={t('common.empty')}
              columns={[
                { id: 'name', label: t('services.name'), rowHeader: true, render: (s) => s.name },
                {
                  id: 'state',
                  label: t('services.active'),
                  render: (s) => (
                    <Badge
                      tone={
                        !s.enabled
                          ? s.state?.ActiveState === 'active'
                            ? 'warning'
                            : 'neutral'
                          : s.state?.ActiveState === 'active'
                            ? 'success'
                            : 'warning'
                      }
                    >
                      {!s.enabled
                        ? s.state?.ActiveState === 'active'
                          ? t('overview.unexpectedService')
                          : t('common.disabled')
                        : (s.state?.ActiveState ?? t('common.unavailable'))}
                    </Badge>
                  ),
                },
              ]}
            />
          </Section>
          <Section title={t('common.current')} contentInset>
            <Details
              label={t('common.current')}
              items={[
                [t('common.current'), d.current ?? '—'],
                [t('common.version'), d.version],
                [t('overview.integrity'), t(d.integrity ? 'common.healthy' : 'common.attention')],
                [
                  t('overview.uptime'),
                  m?.uptime_seconds === null || m?.uptime_seconds === undefined
                    ? '—'
                    : number(m.uptime_seconds / 3600) + ' h',
                ],
              ]}
            />
          </Section>
        </>
      ) : null}
    </NodePage>
  );
}
