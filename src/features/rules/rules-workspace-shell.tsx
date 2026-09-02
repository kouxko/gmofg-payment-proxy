import type { ReactNode } from "react";

export function RulesWorkspaceShell({ children }: { children: ReactNode }) {
  return (
    <section className="grid h-full grid-cols-[minmax(600px,1fr)_560px] max-[1280px]:h-auto max-[1280px]:grid-cols-1">
      {children}
    </section>
  );
}
