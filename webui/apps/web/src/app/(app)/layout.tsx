import type { ReactNode } from 'react';
import { ConsoleShell } from '../../shell/console-shell';
export default function ApplicationLayout({ children }: Readonly<{ children: ReactNode }>) {
  return <ConsoleShell>{children}</ConsoleShell>;
}
