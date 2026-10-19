'use client';
import { useMemo, useState, type ReactNode } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useFrontendTranslation } from '@community-go/i18n';
import { generatedSurfaceRegistry } from '@community-go/surface/generated/composition';
import {
  convertRegistryToShellNavigation,
  navigationIconVocabulary,
} from '@community-go/surface/shell';
import { NavigationIcon } from '@community-go/surface/icon-presentation';
import { DrawerSurface } from '@community-go/ui-adapter/overlays';
import { ShellNavigation, ShellRoot } from '@community-go/surface-foundation/shell-navigation';
import { Action } from '@community-go/ui-adapter/action';
import { SkipLink } from '@community-go/ui-adapter/accessibility';
import { Badge } from '@community-go/ui-adapter/feedback';
import { useConsoleSession } from '../host/console-runtime';
export function ConsoleShell({ children }: Readonly<{ children: ReactNode }>) {
  const { t } = useFrontendTranslation();
  const pathname = usePathname();
  const session = useConsoleSession();
  const groups = useMemo(() => convertRegistryToShellNavigation(generatedSurfaceRegistry), []);
  const [mobile, setMobile] = useState(false);
  const [error, setError] = useState('');
  const path = pathname.replace(/^\/admin(?=\/|$)/, '') || '/dmit';
  const navigation = (close?: () => void) => (
    <ShellNavigation
      groups={groups}
      onNavigate={close}
      router={{
        currentPath: path === '/' ? '/dmit' : path,
        renderLink: (p) => (
          <Link
            href={p.href}
            className={p.className}
            aria-current={p.current ? 'page' : undefined}
            {...(p.onNavigate ? { onClick: p.onNavigate } : {})}
            key={p.href}
          >
            {p.children}
          </Link>
        ),
      }}
      presenter={{
        translate: (key) => t(key),
        icon: (iconId) => {
          const id = navigationIconVocabulary.find((id) => id === iconId);
          return id ? <NavigationIcon iconId={id} /> : null;
        },
      }}
    />
  );
  return (
    <>
      <SkipLink href="#main-content" label={t('nodeShell.skip')} />
      <ShellRoot>
        <aside className="hidden border-r border-border bg-surface p-5 lg:sticky lg:top-0 lg:block lg:h-screen lg:overflow-y-auto">
          <Link href="/dmit" className="flex items-center gap-3 text-ink">
            <span className="grid size-10 place-items-center rounded-control bg-brand text-lg font-bold text-on-brand">
              D
            </span>
            <span>
              <strong className="block text-base">DMIT Console</strong>
              <span className="text-xs text-ink-muted">{t('nodeShell.subtitle')}</span>
            </span>
          </Link>
          <div className="mt-6">{navigation()}</div>
          <div className="mt-6 border-t border-border pt-4">
            <Badge tone="success">{t('nodeShell.safe')}</Badge>
            <p className="mt-3 text-xs leading-6 text-ink-muted">{t('nodeShell.protection')}</p>
          </div>
        </aside>
        <div className="min-w-0">
          <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-surface px-6 py-4">
            <div className="flex items-center gap-3">
              <div className="lg:hidden">
                <Action variant="quiet" onPress={() => setMobile(true)}>
                  {t('nodeShell.navigation')}
                </Action>
                <DrawerSurface
                  triggerLabel={t('nodeShell.navigation')}
                  title="DMIT Console"
                  description={t('nodeShell.navigationDescription')}
                  closeLabel={t('nodeShell.closeNavigation')}
                  isOpen={mobile}
                  onOpenChange={setMobile}
                  composition="navigation"
                  placement="left"
                >
                  {navigation(() => setMobile(false))}
                </DrawerSurface>
              </div>
              <span className="text-sm text-ink-muted">{t('nodeShell.context')}</span>
            </div>
            <div className="flex gap-2">
              <Action variant="quiet" onPress={session.toggleTheme}>
                {t(session.theme === 'light' ? 'nodeShell.dark' : 'nodeShell.light')}
              </Action>
              <Action
                variant="secondary"
                onPress={() => {
                  void session
                    .logout()
                    .catch((e) => setError(e instanceof Error ? e.message : String(e)));
                }}
              >
                {t('nodeShell.logout')}
              </Action>
            </div>
          </header>
          {error ? (
            <p role="alert" className="p-4 text-danger">
              {error}
            </p>
          ) : null}
          <main
            id="main-content"
            className="surface-route-content mx-auto max-w-7xl p-5 sm:p-8"
            tabIndex={-1}
          >
            {children}
          </main>
        </div>
      </ShellRoot>
    </>
  );
}
