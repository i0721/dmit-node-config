import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { ConsoleRuntime } from '../host/console-runtime';
import '../styles.css';
// Next consumes metadata alongside the default layout export.
/* eslint-disable react-refresh/only-export-components */
export const metadata: Metadata = { title: 'DMIT 节点管理', description: 'DMIT 单机节点管理后台' };
export default function RootLayout({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="zh-CN" data-theme="light">
      <body>
        <ConsoleRuntime>{children}</ConsoleRuntime>
      </body>
    </html>
  );
}
