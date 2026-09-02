"use client";

import type { ReactNode } from "react";
import { Card, Tabs } from "@heroui/react";

export type ProtocolType = "http" | "socket";

interface ProtocolWorkspaceTabsProps {
  ariaLabel: string;
  pageTitle?: string;
  selectedKey: ProtocolType;
  onSelectionChange: (protocol: ProtocolType) => void;
  children: ReactNode;
}

/** Shared protocol switch for workspaces with isolated HTTP/Socket state. */
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
      <header className="shrink-0 px-5 pt-5">
        {pageTitle ? (
          <h1 className="mb-4 text-2xl font-semibold">{pageTitle}</h1>
        ) : null}
        <Card className="border border-[var(--telemetry-line)] shadow-sm">
          <Card.Content className="p-0">
            <Tabs.ListContainer>
              <Tabs.List aria-label={ariaLabel} className="px-3 pt-2">
                <Tabs.Tab id="http">
                  HTTP
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="socket">
                  Socket
                  <Tabs.Indicator />
                </Tabs.Tab>
              </Tabs.List>
            </Tabs.ListContainer>
          </Card.Content>
        </Card>
      </header>
      <Tabs.Panel id={selectedKey} className="min-h-0 flex-1">
        {children}
      </Tabs.Panel>
    </Tabs>
  );
}
