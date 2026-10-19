'use client';
/* eslint-disable react-refresh/only-export-components */
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useFrontendTranslation, formatDate, formatNumber } from '@community-go/i18n';
import { Page, PageHeader, Section } from '@community-go/surface-foundation/layout';
import { Action } from '@community-go/ui-adapter/action';
import { StateSurface } from '@community-go/ui-adapter/state-surface';
import { AlertBanner, Badge } from '@community-go/ui-adapter/feedback';
import { ConfirmDialog } from '@community-go/ui-adapter/overlays';
import { DescriptionList } from '@community-go/ui-adapter/description-list';
import {
  useNodeManagement,
  type OperationInput,
  type JobResult,
} from '../../../src/node-management';
export function useNodeText() {
  const { t } = useFrontendTranslation();
  return (key: string) => t('dmit.' + key);
}
export function useNodeQuery<T>(load: () => Promise<T>, interval = 0) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const active = useRef(true);
  const refresh = useCallback(async () => {
    try {
      const value = await load();
      if (active.current) {
        setData(value);
        setError('');
      }
    } catch (e) {
      if (active.current) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (active.current) setLoading(false);
    }
  }, [load]);
  useEffect(() => {
    active.current = true;
    void Promise.resolve().then(refresh);
    const timer = interval
      ? setInterval(() => {
          void refresh();
        }, interval)
      : undefined;
    return () => {
      active.current = false;
      if (timer) clearInterval(timer);
    };
  }, [refresh, interval]);
  return { data, error, loading, refresh };
}
export function NodePage({
  name,
  loading,
  error,
  refresh,
  children,
}: Readonly<{
  name: string;
  loading?: boolean;
  error?: string;
  refresh: () => Promise<void>;
  children: ReactNode;
}>) {
  const t = useNodeText();
  return (
    <Page>
      <PageHeader
        eyebrow="DMIT / NODE CONSOLE"
        title={t(name + '.title')}
        description={t(name + '.description')}
        actions={
          <Action
            variant="secondary"
            onPress={() => {
              void refresh();
            }}
          >
            {t('common.refresh')}
          </Action>
        }
      />
      {loading ? (
        <StateSurface
          state="loading"
          icon="…"
          title={t('common.loading')}
          description={t('common.loadingDescription')}
        />
      ) : null}
      {error ? (
        <AlertBanner
          icon="!"
          tone="danger"
          title={t('common.error')}
          description={error}
          actionLabel={t('common.retry')}
          onAction={() => {
            void refresh();
          }}
          announcement="urgent"
        />
      ) : null}
      {children}
    </Page>
  );
}
export function Details({
  label,
  items,
}: Readonly<{ label: string; items: readonly (readonly [string, ReactNode])[] }>) {
  return (
    <DescriptionList
      label={label}
      columns={2}
      items={items.map(([term, description], i) => ({ id: String(i), term, description }))}
    />
  );
}
export function Code({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-all rounded-control border border-border bg-surface-inset p-4 text-xs leading-6 text-ink">
      {children}
    </pre>
  );
}
export function Timestamp({ value }: Readonly<{ value: number }>) {
  return (
    <time dateTime={new Date(value * 1000).toISOString()}>
      {formatDate('zh-CN', value * 1000, { dateStyle: 'short', timeStyle: 'medium' })}
    </time>
  );
}
export function number(value: number) {
  return formatNumber('zh-CN', value, { maximumFractionDigits: 1 });
}
export function bytes(value: number | null | undefined) {
  if (value === null || value === undefined) return '—';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return number(value) + ' ' + units[i];
}
export function JobProgress() {
  const api = useNodeManagement();
  const t = useNodeText();
  const job = api.job;
  if (!job) return null;
  return (
    <AlertBanner
      icon={job.ok === false ? '!' : '↻'}
      tone={job.ok === false ? 'danger' : job.finished ? 'success' : 'info'}
      title={t(
        job.ok === false ? 'common.failed' : job.finished ? 'common.complete' : 'common.pending',
      )}
      description={job.operation + ' · ' + job.phase + ' · ' + job.id.slice(0, 12)}
      announcement="polite"
    />
  );
}
export function OperationButton({
  label,
  risk,
  input,
  onResult,
  disabled = false,
}: Readonly<{
  label: string;
  risk: string;
  input: Omit<OperationInput, 'confirm'>;
  onResult?: (result: JobResult) => void;
  disabled?: boolean;
}>) {
  const api = useNodeManagement();
  const t = useNodeText();
  const [error, setError] = useState('');
  if (disabled)
    return (
      <Action disabled variant="secondary">
        {label}
      </Action>
    );
  return (
    <div className="grid gap-3">
      <ConfirmDialog
        triggerLabel={label}
        title={label}
        description={risk}
        impact={<Badge tone="warning">{input.service ?? input.operation}</Badge>}
        cancelLabel={t('common.cancel')}
        confirmLabel={t('common.confirm')}
        failureMessage={error || t('common.failed')}
        onConfirm={async () => {
          try {
            const result = await api.operate({ ...input, confirm: input.operation });
            setError('');
            onResult?.(result);
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
            throw e;
          }
        }}
      />
      {error ? (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
export function TaskResult({ result }: Readonly<{ result: JobResult | null }>) {
  const t = useNodeText();
  if (
    !result ||
    (!result.message &&
      !result.token &&
      result.log === undefined &&
      !result.checks &&
      !result.diagnostics &&
      !result.backup)
  )
    return null;
  return (
    <Section title={t('common.details')} contentInset>
      {result.message ? <p className="mb-4 text-sm leading-7">{result.message}</p> : null}
      {result.token ? (
        <div className="grid gap-3">
          <strong>{t('nodes.token')}</strong>
          <Code>{result.token}</Code>
          <p className="text-sm text-warning">{t('nodes.saveToken')}</p>
        </div>
      ) : null}
      {result.log !== undefined ? <Code>{result.log}</Code> : null}
      {result.checks ? (
        <div className="grid gap-3">
          {result.checks.map((check) => (
            <div key={check.name} className="border-b border-border pb-3">
              <Badge tone={check.ok ? 'success' : 'danger'}>{check.name}</Badge>
              <p className="mt-2 text-sm text-ink-muted">
                {check.ok ? check.detail : check.remedy}
              </p>
            </div>
          ))}
        </div>
      ) : null}
      {result.diagnostics ? (
        <Details
          label={t('network.results')}
          items={result.diagnostics.map((d) => [
            d.name,
            <span key={d.name}>
              <Badge tone={d.ok ? 'success' : 'danger'}>
                {t(d.ok ? 'common.healthy' : 'common.attention')}
              </Badge>
              <Code>{d.trace ?? '—'}</Code>
            </span>,
          ])}
        />
      ) : null}
      {result.backup ? <Code>{result.backup}</Code> : null}
    </Section>
  );
}
