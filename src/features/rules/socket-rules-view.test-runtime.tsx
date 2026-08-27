import type { ReactElement } from "react";
import { vi } from "vitest";
import { ProtocolRulesView } from "./protocol-rules-view";
import {
  editorContext,
  httpListener,
  savedFromInput,
  socketListener,
} from "./socket-rules-view.test-support";

const commandMocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  protocolRuleList: vi.fn(),
  protocolRuleEditorContext: vi.fn(),
  protocolRuleSave: vi.fn(),
  protocolRuleToggle: vi.fn(),
  protocolRuleDelete: vi.fn(),
  protocolRuleParseValue: vi.fn(),
}));

const queryState = vi.hoisted(() => ({
  listeners: [] as unknown[],
  rules: [] as unknown[],
  capabilityError: undefined as string | undefined,
  blockedSource: undefined as undefined | "workspaces" | "workspace" | "rules",
  blockedState: "loading" as "loading" | "error",
  capabilities: new Map<string, unknown>(),
  refresh: vi.fn(),
  eventRefresh: undefined as undefined | (() => Promise<void>),
}));

vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: (_events: unknown, refresh: () => Promise<void>) => {
    queryState.eventRefresh = refresh;
  },
}));
vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: (reason: unknown) => reason && typeof reason === "object" ? reason : undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 操作失败",
}));

function querySource(key: string): "workspaces" | "workspace" | "rules" | undefined {
  if (key === "protocol-rule-workspaces") {
    return "workspaces";
  }
  if (key.startsWith("protocol-rule-workspace:")) {
    return "workspace";
  }
  if (key.startsWith("protocol-rule-list:")) {
    return "rules";
  }
  return undefined;
}

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => {
    let data: unknown;
    let error: string | undefined;
    const source = querySource(key);
    if (key === "protocol-rule-workspaces") {
      data = [{
        id: "workspace-1",
        name: "工作区",
        revision: 1,
        listener_count: queryState.listeners.length,
        enabled_listener_count: queryState.listeners.length,
        selected: true,
      }];
    } else if (key.startsWith("protocol-rule-workspace:")) {
      data = {
        id: "workspace-1",
        name: "工作区",
        revision: 1,
        listeners: queryState.listeners,
        metadata_extractors: [],
        response_assertions: [],
        fault_presets: [],
        certificate_references: [],
      };
    } else if (key.startsWith("protocol-rule-list:")) {
      data = queryState.rules;
    } else if (key.startsWith("protocol-rule-editor-context:")) {
      data = queryState.capabilities.get(key);
      error = queryState.capabilityError;
    }
    const blocked = source != null && source === queryState.blockedSource;
    if (blocked && queryState.blockedState === "error") {
      error = "事实源不可用";
    }
    return {
      data,
      error,
      isLoading: blocked && queryState.blockedState === "loading",
      refresh: queryState.refresh,
      invalidate: vi.fn(),
    };
  },
}));

export function getCommandMocks(): typeof commandMocks {
  return commandMocks;
}

export function getQueryState(): typeof queryState {
  return queryState;
}

export function SocketRulesView(): ReactElement {
  return <ProtocolRulesView kind="socket" />;
}

export function HttpRulesView(): ReactElement {
  return <ProtocolRulesView kind="http" />;
}

function installCapabilities(): void {
  const allStages = ["app_to_proxy", "proxy_to_upstream", "upstream_to_proxy", "proxy_to_app"] as const;
  queryState.capabilities.set(
    "protocol-rule-editor-context:relay",
    editorContext("relay", allStages.map((stage) => ({ stage }))),
  );
  queryState.capabilities.set(
    "protocol-rule-editor-context:local",
    editorContext("local", [
      { stage: "app_to_proxy", schemaVersion: 8 },
      { stage: "proxy_to_app", schemaVersion: 8 },
    ], "clear_document"),
  );
  queryState.capabilities.set(
    "protocol-rule-editor-context:http",
    editorContext("http", allStages.map((stage) => ({ stage }))),
  );
}

export function resetSocketRulesViewTestState(): void {
  vi.clearAllMocks();
  queryState.listeners = [
    socketListener("relay"),
    socketListener("local", "local"),
    socketListener("direct", "direct"),
    httpListener(),
    httpListener("plain-http", false),
  ];
  queryState.rules = [];
  queryState.capabilityError = undefined;
  queryState.blockedSource = undefined;
  queryState.blockedState = "loading";
  queryState.capabilities.clear();
  installCapabilities();
  queryState.refresh.mockResolvedValue(undefined);
  queryState.eventRefresh = undefined;
  commandMocks.protocolRuleParseValue.mockImplementation(async (type: string, raw: string) => {
    if (type === "string") {
      return { type, value: raw };
    }
    if (type === "int") {
      return { type, value: Number(raw) };
    }
    if (type === "bool") {
      return { type, value: raw === "true" };
    }
    return { type, value: [] };
  });
  commandMocks.protocolRuleSave.mockImplementation(
    async (input: Record<string, unknown>) => savedFromInput(input),
  );
}
