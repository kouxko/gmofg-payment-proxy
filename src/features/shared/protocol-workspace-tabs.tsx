"use client";

import type { ReactNode } from "react";
import { Tabs } from "@heroui/react";

export type ProtocolType = "http" | "socket";

interface ProtocolWorkspaceTabsProps {
  ariaLabel: string;
  pageTitle: string;
  selectedKey: ProtocolType;
  onSelectionChange: (protocol: ProtocolType) => void;
  children: ReactNode;
}

/** Shared compact protocol switch for workspaces with isolated HTTP/Socket state. */
export function ProtocolWorkspaceTabs({
  ariaLabel,
  pageTitle,
  selectedKey,
  onSelectionChange,
  children,
}: ProtocolWorkspaceTabsProps) {
  return (
    <Tabs
      className="flex h-full min-h-0 flex-col"
      selectedKey={selectedKey}
      onSelectionChange={(key) => {
        if (key === "http" || key === "socket") onSelectionChange(key);
      }}
    >
      <header className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-[var(--telemetry-line)] px-5 py-3">
        <h1 className="text-2xl font-semibold">{pageTitle}</h1>
        <Tabs.ListContainer className="w-fit">
          <Tabs.List
            aria-label={ariaLabel}
            className="w-fit rounded-lg bg-[var(--telemetry-soft)] p-1"
          >
            <Tabs.Tab id="http" className="min-w-0 px-3 py-1.5 text-sm">
              HTTP
              <Tabs.Indicator />
            </Tabs.Tab>
            <Tabs.Tab id="socket" className="min-w-0 px-3 py-1.5 text-sm">
              Socket
              <Tabs.Indicator />
            </Tabs.Tab>
          </Tabs.List>
        </Tabs.ListContainer>
      </header>
      <Tabs.Panel id={selectedKey} className="min-h-0 flex-1">
        {children}
      </Tabs.Panel>
    </Tabs>
  );
}
