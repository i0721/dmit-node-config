'use client';
import { useEffect } from 'react';
import { useRouter } from 'next/navigation';
import { StateSurface } from '@community-go/ui-adapter/state-surface';
import { useFrontendTranslation } from '@community-go/i18n';
export default function ConsoleEntry() {
  const router = useRouter();
  const { t } = useFrontendTranslation();
  useEffect(() => router.replace('/dmit'), [router]);
  return (
    <StateSurface
      state="loading"
      icon="…"
      title={t('nodeShell.restoring')}
      description={t('nodeShell.restoreDescription')}
    />
  );
}
