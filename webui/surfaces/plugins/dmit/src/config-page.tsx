'use client';
import { useState, lazy, Suspense } from 'react';
const PlanReview = lazy(() => import('./plan-review'));
import { Section } from '@community-go/surface-foundation/layout';
import { Action } from '@community-go/ui-adapter/action';
import { TextField, TextAreaField, PasswordField } from '@community-go/ui-adapter/form-field';
import { AlertBanner, Badge } from '@community-go/ui-adapter/feedback';
import {
  useNodeManagement,
  nodeConfigurationSchema,
  type ConfigurationView,
  type ConfigPlan,
} from '../../../src/node-management';
import { NodePage, Details, JobProgress, useNodeQuery, useNodeText } from './common';
function ConfigEditor({
  view,
  refresh,
}: Readonly<{ view: ConfigurationView; refresh: () => Promise<void> }>) {
  const api = useNodeManagement();
  const t = useNodeText();
  const [draft, setDraft] = useState(view.config);
  const [aliases, setAliases] = useState(view.config.node.aliases.join('\n'));
  const [token, setToken] = useState('');
  const [ticket, setTicket] = useState<string | undefined>();
  const [advanced, setAdvanced] = useState(false);
  const [raw, setRaw] = useState('');
  const [plan, setPlan] = useState<ConfigPlan | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const changed = () => {
    setPlan(null);
    setError('');
  };
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
  const preview = () =>
    run(async () => {
      let candidate = draft;
      if (advanced) {
        try {
          candidate = nodeConfigurationSchema.parse(JSON.parse(raw));
        } catch {
          throw new Error(t('config.parseError'));
        }
      }
      const grant = ticket;
      setTicket(undefined);
      const next = await api.preview(candidate, grant);
      setDraft(candidate);
      setPlan(next);
    });
  return (
    <>
      <Section title={t('config.assets')} description={t('config.assetsDescription')} contentInset>
        <div className="grid gap-5 md:grid-cols-2">
          {(['domain', 'reality_domain', 'ipv4', 'ipv6'] as const).map((field) => (
            <TextField
              key={field}
              label={t('config.' + (field === 'reality_domain' ? 'reality' : field))}
              value={draft.node[field]}
              disabled={advanced || pending}
              onChange={(e) => {
                changed();
                setDraft({ ...draft, node: { ...draft.node, [field]: e.target.value } });
              }}
            />
          ))}
        </div>
        <div className="mt-5">
          <TextAreaField
            label={t('config.aliases')}
            hint={t('config.aliasHint')}
            rows={3}
            value={aliases}
            disabled={advanced || pending}
            onChange={(e) => {
              setAliases(e.target.value);
              changed();
              setDraft({
                ...draft,
                node: {
                  ...draft.node,
                  aliases: e.target.value
                    .split('\n')
                    .map((v) => v.trim())
                    .filter(Boolean),
                },
              });
            }}
          />
        </div>
      </Section>
      <Section
        title={t('config.baseline')}
        description={t('config.baselineDescription')}
        contentInset
      >
        <Details
          label={t('config.baseline')}
          items={[
            ['TLS', view.config.tls.protocols.join(', ')],
            [
              'HTTP/2 · HTTP/3',
              String(view.config.tls.http2) + ' · ' + String(view.config.tls.http3),
            ],
            [
              'HTTP / HTTPS / REALITY',
              [view.config.ports.http, view.config.ports.https, view.config.ports.reality].join(
                ' / ',
              ),
            ],
            [t('network.warp'), t(view.config.warp.enabled ? 'common.enabled' : 'common.disabled')],
            [
              t('network.cn'),
              t(view.config.cn_warp?.enabled ? 'common.enabled' : 'common.disabled'),
            ],
            [
              t('dns.certificates'),
              view.config.tls.domain_cert_dir + ' · ' + view.config.tls.ip_cert_dir,
            ],
          ]}
        />
      </Section>
      <Section title={t('config.advanced')} description={t('config.advancedRisk')} contentInset>
        <div className="grid gap-4">
          <PasswordField
            label={t('config.adminToken')}
            purpose="current"
            showLabel="显示"
            hideLabel="隐藏"
            value={token}
            disabled={pending}
            onChange={(e) => setToken(e.target.value)}
          />
          <div>
            <Action
              variant="secondary"
              disabled={!token || pending}
              loading={pending}
              onPress={() => {
                void run(async () => {
                  const grant = await api.unlock(token);
                  setToken('');
                  setTicket(grant);
                  if (!advanced) setRaw(JSON.stringify(draft, null, 2));
                  setAdvanced(true);
                  changed();
                }).catch(() => {});
              }}
            >
              {t('config.unlock')}
            </Action>
          </div>
          {advanced ? (
            <TextAreaField
              label={t('config.advancedConfig')}
              hint={t('config.advancedHint')}
              rows={16}
              value={raw}
              disabled={pending}
              onChange={(e) => {
                setRaw(e.target.value);
                changed();
              }}
            />
          ) : (
            <Badge>{t('config.locked')}</Badge>
          )}
        </div>
      </Section>
      <div className="flex flex-wrap gap-3">
        <Action
          loading={pending}
          disabled={pending || (advanced && !ticket)}
          onPress={() => {
            void preview().catch(() => {});
          }}
        >
          {t('common.preview')}
        </Action>
        {advanced && !ticket ? (
          <p className="text-sm text-ink-muted">{t('config.unlock')}</p>
        ) : null}
      </div>
      {error ? (
        <AlertBanner
          icon="!"
          tone="danger"
          title={t('common.failed')}
          description={error}
          announcement="urgent"
        />
      ) : null}
      {plan ? (
        <Suspense fallback={<p role="status">{t('common.loading')}</p>}>
          <PlanReview
            key={plan.plan}
            plan={plan}
            onApplied={async () => {
              setPlan(null);
              await refresh();
            }}
          />
        </Suspense>
      ) : null}
    </>
  );
}
export default function ConfigPage() {
  const api = useNodeManagement();
  const q = useNodeQuery(api.configuration);
  return (
    <NodePage name="config" loading={q.loading} error={q.error} refresh={q.refresh}>
      <JobProgress />
      {q.data ? <ConfigEditor key={q.data.base} view={q.data} refresh={q.refresh} /> : null}
    </NodePage>
  );
}
