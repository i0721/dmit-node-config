'use client';
import { useState } from 'react';
import { Section } from '@community-go/surface-foundation/layout';
import { Action } from '@community-go/ui-adapter/action';
import { TextField, SwitchField } from '@community-go/ui-adapter/form-field';
import { ConfirmDialog } from '@community-go/ui-adapter/overlays';
import { AlertBanner, Badge } from '@community-go/ui-adapter/feedback';
import { DataTable } from '@community-go/ui-adapter/data-display';
import { useNodeManagement, type ConfigPlan, type JobResult } from '../../../src/node-management';
import { Details, Code, useNodeText } from './common';
export default function PlanReview({
  plan,
  onApplied,
  initialManualCertificates = true,
}: Readonly<{
  plan: ConfigPlan;
  onApplied: () => Promise<void>;
  initialManualCertificates?: boolean;
}>) {
  const api = useNodeManagement();
  const t = useNodeText();
  const [confirm, setConfirm] = useState('');
  const [checked, setChecked] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [result, setResult] = useState<JobResult | null>(null);
  const [manual, setManual] = useState(initialManualCertificates);
  const [skip, setSkip] = useState(true);
  const [email, setEmail] = useState('');
  const [ca, setCa] = useState(false);
  const [warp, setWarp] = useState(false);
  const run = async (work: () => Promise<void>) => {
    setPending(true);
    setError('');
    try {
      await work();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      throw e;
    } finally {
      setPending(false);
    }
  };
  const preparationChanged = () => {
    setChecked(false);
    setResult(null);
  };
  return (
    <>
      {error ? (
        <AlertBanner icon="!" tone="danger" title={t('common.failed')} description={error} />
      ) : null}
      {plan ? (
        <>
          <Section title={t('config.review')} description={plan.risk}>
            <div className="p-5">
              <Details
                label={t('config.review')}
                items={[
                  [
                    t('config.plan'),
                    <span className="break-all font-mono text-xs" key="plan">
                      {plan.plan}
                    </span>,
                  ],
                  [t('common.safe'), t(plan.advanced ? 'common.protected' : 'common.asset')],
                ]}
              />
            </div>
            <DataTable
              label={t('config.review')}
              rows={plan.changes.map((c) => ({ ...c, id: c.field }))}
              emptyContent={t('common.empty')}
              columns={[
                {
                  id: 'field',
                  label: t('common.field'),
                  rowHeader: true,
                  render: (c) => (
                    <span className="break-all">
                      {c.field}
                      <span className="ml-2">
                        <Badge tone={c.protected ? 'warning' : 'info'}>
                          {t(c.protected ? 'common.protected' : 'common.asset')}
                        </Badge>
                      </span>
                    </span>
                  ),
                },
                {
                  id: 'before',
                  label: t('common.before'),
                  render: (c) => (
                    <code className="break-all text-xs">{JSON.stringify(c.before)}</code>
                  ),
                },
                {
                  id: 'after',
                  label: t('common.after'),
                  render: (c) => (
                    <code className="break-all text-xs">{JSON.stringify(c.after)}</code>
                  ),
                },
              ]}
            />
            <div className="p-5">
              <h3 className="mb-3 font-semibold">{t('config.diff')}</h3>
              <Code>{plan.diff}</Code>
            </div>
          </Section>
          <Section
            title={t('config.deployment')}
            description={t('config.deploymentDescription')}
            contentInset
          >
            <div className="grid gap-4 md:grid-cols-2">
              <SwitchField
                label={t('config.manualCertificates')}
                description=""
                checked={manual}
                onCheckedChange={(v) => {
                  setManual(v);
                  preparationChanged();
                }}
              />
              <SwitchField
                label={t('config.skipDependencies')}
                description=""
                checked={skip}
                onCheckedChange={(v) => {
                  setSkip(v);
                  preparationChanged();
                }}
              />
              <SwitchField
                label={t('config.caTerms')}
                description=""
                checked={ca}
                onCheckedChange={(v) => {
                  setCa(v);
                  preparationChanged();
                }}
              />
              <SwitchField
                label={t('config.warpTerms')}
                description=""
                checked={warp}
                onCheckedChange={(v) => {
                  setWarp(v);
                  preparationChanged();
                }}
              />
            </div>
            <div className="mt-4">
              <TextField
                label={t('config.email')}
                value={email}
                onChange={(e) => {
                  setEmail(e.target.value);
                  preparationChanged();
                }}
              />
            </div>
            <div className="mt-5 flex flex-wrap gap-3">
              <Action
                variant="secondary"
                disabled={pending}
                onPress={() => {
                  void run(async () => {
                    preparationChanged();
                    const r = await api.check(plan.plan);
                    setResult(r);
                    setChecked(true);
                  }).catch(() => {});
                }}
              >
                {t('common.check')}
              </Action>
              <Action
                variant="secondary"
                disabled={pending || plan.restores_identity}
                onPress={() => {
                  void run(async () => {
                    preparationChanged();
                    const r = await api.prepareDeployment(plan.plan, {
                      manual_certificates: manual,
                      skip_dependencies: skip,
                      accept_ca_terms: ca,
                      accept_warp_terms: warp,
                      ...(email ? { acme_email: email } : {}),
                    });
                    setResult(r);
                    setChecked(true);
                  }).catch(() => {});
                }}
              >
                {t('config.prepareDeployment')}
              </Action>
            </div>
          </Section>
          {typeof result?.deployment === 'object' ? (
            <Section title={t('config.deployPlan')} contentInset>
              <Details
                label={t('config.deployPlan')}
                items={[
                  [t('config.plan'), result.deployment_digest],
                  [t('common.phase'), result.deployment.stages.join(' → ')],
                ]}
              />
              <ul className="mt-4 list-disc space-y-2 pl-5 text-sm text-warning">
                {result.deployment.recovery_limits.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
              {result.deployment.dns_changes.length ? (
                <div className="mt-4">
                  <Code>{JSON.stringify(result.deployment.dns_changes, null, 2)}</Code>
                </div>
              ) : null}
            </Section>
          ) : null}
          <Section title={t('common.apply')} description={t('config.confirmHint')} contentInset>
            <div className="grid gap-4">
              {checked ? (
                <Badge tone="success">
                  {t(
                    result?.check_deferred_until_dependencies
                      ? 'config.prepareDeployment'
                      : 'config.checked',
                  )}
                </Badge>
              ) : null}
              <TextField
                label={t('config.confirmation')}
                value={confirm}
                disabled={!checked || pending}
                onChange={(e) => setConfirm(e.target.value)}
              />
              <ConfirmDialog
                triggerLabel={t('common.apply')}
                title={t('common.apply')}
                description={t('config.applyRisk')}
                impact={<Code>{plan.plan}</Code>}
                cancelLabel={t('common.cancel')}
                confirmLabel={t('common.confirm')}
                confirmDisabled={!checked || confirm !== plan.plan || pending}
                failureMessage={error || t('common.failed')}
                tone="danger"
                onConfirm={() =>
                  run(async () => {
                    await api.apply(plan.plan, confirm);
                    await onApplied();
                  })
                }
              />
            </div>
          </Section>
        </>
      ) : null}
    </>
  );
}
