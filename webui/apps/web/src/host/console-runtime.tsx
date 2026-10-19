'use client';
/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  createFrontendI18n,
  FrontendI18nProvider,
  useFrontendTranslation,
} from '@community-go/i18n';
import { NodeManagementProvider, type ManagedJob } from '@community-go/surface/node-management';
import { generatedSurfaceI18nResources } from '@community-go/surface/generated/composition';
import {
  MotionPolicyContext,
  type MotionPolicyController,
} from '@community-go/surface-foundation/motion-policy-context';
import { StateSurface } from '@community-go/ui-adapter/state-surface';
import { Action } from '@community-go/ui-adapter/action';
import { PasswordField } from '@community-go/ui-adapter/form-field';
import { Panel } from '@community-go/ui-adapter/panel';
import { AlertBanner } from '@community-go/ui-adapter/feedback';
import { createAdminApi } from './api';
import { shellResources } from '../i18n/shell';
const i18n = createFrontendI18n({
  resources: generatedSurfaceI18nResources,
  defaultLocale: 'zh-CN',
  supportedLocales: ['zh-CN', 'en'],
});
i18n.addResources(shellResources);
type SessionPort = Readonly<{
  logout: () => Promise<void>;
  theme: 'light' | 'dark';
  toggleTheme: () => void;
}>;
const SessionContext = createContext<SessionPort | null>(null);
export function useConsoleSession() {
  const s = useContext(SessionContext);
  if (!s) throw new Error('Console session unavailable');
  return s;
}
function Login({ login }: Readonly<{ login: (token: string) => Promise<void> }>) {
  const { t } = useFrontendTranslation();
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const submit = async () => {
    setPending(true);
    setError('');
    try {
      await login(token);
      setToken('');
    } catch (e) {
      setError(e instanceof Error ? e.message : '登录失败');
    } finally {
      setPending(false);
    }
  };
  return (
    <main className="grid min-h-screen place-items-center bg-canvas p-6">
      <Panel className="w-full max-w-md p-8">
        <p className="text-sm font-bold tracking-widest text-brand">DMIT · NODE CONSOLE</p>
        <h1 className="mt-4 text-3xl font-bold text-ink">{t('nodeShell.loginTitle')}</h1>
        <p className="mt-3 text-sm leading-7 text-ink-muted">{t('nodeShell.loginDescription')}</p>
        <form
          className="mt-6 grid gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <PasswordField
            label={t('nodeShell.token')}
            purpose="current"
            showLabel={t('nodeShell.show')}
            hideLabel={t('nodeShell.hide')}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            disabled={pending}
          />
          {error ? (
            <AlertBanner
              icon="!"
              tone="danger"
              title={t('nodeShell.loginFailed')}
              description={error}
              announcement="urgent"
            />
          ) : null}
          <Action type="submit" loading={pending} disabled={!token || pending} fullWidth>
            {t('nodeShell.login')}
          </Action>
        </form>
        <p className="mt-6 text-xs leading-6 text-ink-muted">{t('nodeShell.loginFooter')}</p>
      </Panel>
    </main>
  );
}
export function ConsoleRuntime({ children }: Readonly<{ children: ReactNode }>) {
  const [ready, setReady] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [initialError, setInitialError] = useState('');
  const [job, setJob] = useState<ManagedJob | null>(null);
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const [reduced, setReduced] = useState(false);
  const api = useMemo(
    () =>
      createAdminApi(() => {
        setAuthenticated(false);
        setJob(null);
      }, setJob),
    [],
  );
  useEffect(() => {
    let active = true;
    api
      .session()
      .then(() => {
        if (active) setAuthenticated(true);
      })
      .catch((e) => {
        if (active && e instanceof Error && !('code' in e && e.code === 'unauthorized'))
          setInitialError(e.message);
      })
      .finally(() => {
        if (active) setReady(true);
      });
    return () => {
      active = false;
    };
  }, [api]);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  useEffect(() => {
    const q = window.matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(q.matches);
    update();
    q.addEventListener('change', update);
    return () => q.removeEventListener('change', update);
  }, []);
  const policy = useMemo<MotionPolicyController>(
    () => ({
      mode: 'system',
      scale: 1,
      resolvedMode: reduced ? 'reduced' : 'full',
      categories: {
        screen: true,
        async: true,
        reveal: true,
        swap: true,
        feedback: true,
        media: true,
      },
      inspectorAvailable: false,
      setMode: () => {},
      setScale: () => {},
      setCategories: () => {},
    }),
    [reduced],
  );
  const session = useMemo<SessionPort>(
    () => ({
      logout: () => api.logout(),
      theme,
      toggleTheme: () => setTheme((t) => (t === 'light' ? 'dark' : 'light')),
    }),
    [api, theme],
  );
  const port = useMemo(() => ({ ...api.port, job }), [api, job]);
  return (
    <FrontendI18nProvider runtime={i18n}>
      <MotionPolicyContext.Provider value={policy}>
        <SessionContext.Provider value={session}>
          <NodeManagementProvider port={port}>
            {!ready ? (
              <StateSurface
                state="loading"
                icon="…"
                title={i18n.translate('nodeShell.restoring')}
                description={i18n.translate('nodeShell.restoreDescription')}
              />
            ) : !authenticated ? (
              <>
                <Login
                  login={async (token) => {
                    await api.login(token);
                    setInitialError('');
                    setAuthenticated(true);
                  }}
                />
                {initialError ? (
                  <p role="alert" className="p-4 text-center text-danger">
                    {initialError}
                  </p>
                ) : null}
              </>
            ) : (
              children
            )}
          </NodeManagementProvider>
        </SessionContext.Provider>
      </MotionPolicyContext.Provider>
    </FrontendI18nProvider>
  );
}
