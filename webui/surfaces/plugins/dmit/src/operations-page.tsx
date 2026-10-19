'use client';
import { useState, lazy, Suspense } from 'react';
const PlanReview = lazy(() => import('./plan-review'));
import { Section } from '@community-go/surface-foundation/layout';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { TextField, PasswordField } from '@community-go/ui-adapter/form-field';
import { Action } from '@community-go/ui-adapter/action';
import { ConfirmDialog } from '@community-go/ui-adapter/overlays';
import { AlertBanner, Badge } from '@community-go/ui-adapter/feedback';
import { useNodeManagement, type JobResult, type ConfigPlan } from '../../../src/node-management';
import {
  NodePage,
  Details,
  OperationButton,
  TaskResult,
  JobProgress,
  Timestamp,
  useNodeQuery,
  useNodeText,
} from './common';
export default function OperationsPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.operations, 5000);
  const t = useNodeText();
  const [result, setResult] = useState<JobResult | null>(null);
  const [version, setVersion] = useState('');
  const [restore, setRestore] = useState<ConfigPlan | null>(null);
  const [selected, setSelected] = useState('');
  const [token, setToken] = useState('');
  const [restoreError, setRestoreError] = useState('');
  const d = q.data;
  const done = (r: JobResult) => {
    setResult(r);
    void q.refresh();
  };
  return (
    <NodePage name="operations" loading={q.loading} error={q.error} refresh={q.refresh}>
      <JobProgress />
      {d?.recovery_required ? (
        <AlertBanner
          icon="!"
          tone="danger"
          title={t('operations.recovery')}
          description={t('overview.recoveryDescription')}
        />
      ) : null}
      <Section title={t('operations.actions')} contentInset>
        <div className="flex flex-wrap gap-3">
          <OperationButton
            label={t('operations.doctor')}
            risk={t('operations.doctorRisk')}
            input={{ operation: 'doctor' }}
            onResult={done}
          />
          <OperationButton
            label={t('operations.restartApi')}
            risk={t('operations.restartApiRisk')}
            input={{ operation: 'api-restart' }}
            onResult={done}
          />
          <OperationButton
            label={t('common.backup')}
            risk={t('operations.backupRisk')}
            input={{ operation: 'backup' }}
            onResult={done}
          />
          <OperationButton
            label={t('operations.rollback')}
            risk={t('operations.rollbackRisk')}
            input={{ operation: 'rollback' }}
            disabled={!d?.previous}
            onResult={done}
          />
          <OperationButton
            label={t('operations.recover')}
            risk={t('operations.recoverRisk')}
            input={{ operation: 'recover' }}
            disabled={!d?.recovery_required}
            onResult={done}
          />
          <OperationButton
            label={t('operations.logs')}
            risk={t('operations.logsRisk')}
            input={{ operation: 'logs' }}
            onResult={done}
          />
        </div>
        <div className="mt-5">
          <Details
            label={t('common.current')}
            items={[
              [t('common.current'), d?.current ?? '—'],
              [t('common.previous'), d?.previous ?? t('operations.noPrevious')],
            ]}
          />
        </div>
      </Section>
      <Section
        title={t('operations.upgrade')}
        description={t('operations.upgradeRisk')}
        contentInset
      >
        <div className="grid gap-4">
          <TextField
            label={t('operations.upgradeVersion')}
            value={version}
            onChange={(e) => setVersion(e.target.value)}
          />
          <OperationButton
            label={t('operations.upgrade')}
            risk={t('operations.upgradeRisk')}
            input={{ operation: 'upgrade', version }}
            disabled={!/^v?\d+\.\d+\.\d+$/.test(version)}
            onResult={done}
          />
        </div>
      </Section>
      <TaskResult result={result} />
      {restore ? (
        <Suspense fallback={<p role="status">{t('common.loading')}</p>}>
          <PlanReview
            key={restore.plan}
            plan={restore}
            onApplied={async () => {
              setRestore(null);
              await q.refresh();
            }}
          />
        </Suspense>
      ) : null}
      <ConfirmDialog
        triggerLabel={t('operations.restore')}
        title={t('operations.restore')}
        description={t('operations.restoreRisk')}
        impact={
          <div className="grid gap-4">
            <p className="break-all font-mono text-xs">{selected}</p>
            <PasswordField
              label={t('config.adminToken')}
              purpose="current"
              showLabel={t('common.show')}
              hideLabel={t('common.hide')}
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
          </div>
        }
        isOpen={Boolean(selected)}
        onOpenChange={(open) => {
          if (!open) {
            setSelected('');
            setToken('');
          }
        }}
        cancelLabel={t('common.cancel')}
        confirmLabel={t('common.preview')}
        confirmDisabled={!token}
        failureMessage={restoreError || t('common.failed')}
        tone="danger"
        onConfirm={async () => {
          try {
            const plan = await api.restoreBackup(selected, token);
            setToken('');
            setRestore(plan);
            setSelected('');
            setRestoreError('');
          } catch (e) {
            setRestoreError(e instanceof Error ? e.message : String(e));
            throw e;
          }
        }}
      />
      <Section title={t('operations.history')}>
        <DataTable
          label={t('operations.history')}
          rows={(d?.jobs ?? []).map((j) => ({ ...j }))}
          emptyContent={t('operations.noJobs')}
          columns={[
            {
              id: 'time',
              label: t('operations.time'),
              render: (j) => <Timestamp value={j.started} />,
            },
            {
              id: 'operation',
              label: t('operations.operation'),
              rowHeader: true,
              render: (j) => j.operation,
            },
            {
              id: 'phase',
              label: t('common.phase'),
              render: (j) => (
                <Badge tone={j.ok === false ? 'danger' : j.finished ? 'success' : 'info'}>
                  {j.phase}
                </Badge>
              ),
            },
          ]}
        />
      </Section>
      <Section title={t('operations.backups')}>
        <DataTable
          label={t('operations.backups')}
          rows={d?.backups ?? []}
          emptyContent={t('operations.noBackups')}
          columns={[
            {
              id: 'id',
              label: t('operations.id'),
              rowHeader: true,
              render: (b) => <span className="break-all font-mono text-xs">{b.id}</span>,
            },
            {
              id: 'created',
              label: t('operations.time'),
              render: (b) => <Timestamp value={b.created} />,
            },
            {
              id: 'restore',
              label: t('services.actions'),
              render: (b) => (
                <Action
                  variant="secondary"
                  disabled={!b.restorable}
                  onPress={() => {
                    setSelected(b.id);
                    setRestoreError('');
                  }}
                >
                  {t(b.restorable ? 'operations.restore' : 'operations.invalidSnapshot')}
                </Action>
              ),
            },
          ]}
        />
      </Section>
      <Section title={t('operations.audit')}>
        <DataTable
          label={t('operations.audit')}
          rows={(d?.audit ?? []).map((a, i) => ({ ...a, id: String(i) }))}
          emptyContent={t('common.empty')}
          columns={[
            {
              id: 'time',
              label: t('operations.time'),
              render: (a) => <Timestamp value={a.time} />,
            },
            {
              id: 'op',
              label: t('operations.operation'),
              rowHeader: true,
              render: (a) => a.operation,
            },
            { id: 'outcome', label: t('operations.outcome'), render: (a) => a.outcome },
          ]}
        />
      </Section>
    </NodePage>
  );
}
