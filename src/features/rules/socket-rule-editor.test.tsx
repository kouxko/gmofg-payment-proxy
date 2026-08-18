// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ProxyListener,
  ProtocolRuleStage,
  ProtocolRuleCapabilityCatalog,
} from "@/generated/rust-types";
import { ProtocolRuleEditor } from "./socket-rule-editor";
import {
  newProtocolRuleDraft,
  type ProtocolRuleDraft,
} from "./socket-rule-model";

const commandMocks = vi.hoisted(() => ({ protocolRuleParseValue: vi.fn() }));
vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 解析失败",
}));

beforeEach(() => {
  commandMocks.protocolRuleParseValue.mockImplementation(async (type: string, raw: string) => {
    if (type === "string") return { type, value: raw };
    if (type === "int") return { type, value: Number(raw) };
    if (type === "bool") return { type, value: raw === "true" };
    const compact = raw.replace(/\s/g, "");
    return { type, value: compact.match(/.{2}/g)?.map((byte) => Number.parseInt(byte, 16)) ?? [] };
  });
});

const packageRef = { id: "iso8583", version: "1.2.3" };

function listener(id: string, local = false): ProxyListener {
  return {
    id,
    name: local ? "本地应答" : "交易中继",
    enabled: true,
    bind_address: "127.0.0.1",
    port: local ? 9002 : 9001,
    allowed_client_cidrs: [],
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
    data_plane: {
      kind: "socket",
      settings: {
        topology: local
          ? { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } }
          : {
              mode: "relay",
              settings: {
                upstream: { host: "example.test", port: 9000 },
                security: { mode: "transparent" },
              },
            },
        maximum_connections: 8,
        processing: {
          mode: "scripted",
          settings: {
            package: packageRef,
          },
        },
      },
    },
  };
}

const fields: ProtocolRuleCapabilityCatalog["fields"] = [
  { name: "message_type", label: "消息类型", type: "string", operators: ["equals"], actions: ["set_field"] },
  { name: "amount", label: "金额", type: "int", operators: ["equals"], actions: ["set_field"] },
  { name: "approved", label: "批准", type: "bool", operators: ["equals"], actions: ["set_field"] },
  { name: "bitmap", label: "位图", type: "blob", operators: ["equals"], actions: ["set_field"] },
];

function catalog(stage: ProtocolRuleStage = "app_to_proxy"): ProtocolRuleCapabilityCatalog {
  return {
    package: packageRef,
    schema_version: 7,
    stage,
    fields,
    common_actions: ["record_match", "clear_document"],
  };
}

function Harness({
  activeListener = listener("relay"),
  activeCatalog = catalog(),
  creating = true,
  initialDraft,
  loading = false,
  error,
  fieldErrors = {},
  pending = false,
  invalidInitially = false,
  onSave = vi.fn(),
  onDelete = vi.fn(),
  onReload = vi.fn(),
}: {
  activeListener?: ProxyListener;
  activeCatalog?: ProtocolRuleCapabilityCatalog;
  creating?: boolean;
  initialDraft?: ProtocolRuleDraft;
  loading?: boolean;
  error?: string;
  fieldErrors?: Record<string, string[]>;
  pending?: boolean;
  invalidInitially?: boolean;
  onSave?: (draft: ProtocolRuleDraft) => void;
  onDelete?: () => void;
  onReload?: () => void;
}) {
  const [draft, setDraft] = useState(
    initialDraft ?? newProtocolRuleDraft(activeListener, activeCatalog.stage, activeCatalog),
  );
  const [valueStates, setValueStates] = useState<Record<string, { pending: boolean; invalid: boolean }>>(
    invalidInitially ? { invalid: { pending: false, invalid: true } } : {},
  );
  return <ProtocolRuleEditor
    catalog={activeCatalog}
    creating={creating}
    draft={draft}
    error={error}
    fieldErrors={fieldErrors}
    valueStates={valueStates}
    listener={activeListener}
    listeners={[listener("relay"), listener("local", true)]}
    loading={loading}
    onChange={setDraft}
    onDelete={onDelete}
    onStageChange={(stage) => setDraft({ ...draft, stage })}
    onValueStateChange={(key, state) => setValueStates((current) => {
      const next = { ...current };
      if (state) next[key] = state; else delete next[key];
      return next;
    })}
    onListenerChange={vi.fn()}
    onReload={onReload}
    onReloadRule={vi.fn()}
    onResetInvalidValues={() => setValueStates({})}
    onSave={() => onSave(draft)}
    pending={pending}
  />;
}

describe("Socket rule editor product boundary", () => {
  it("shows a capability loading state", () => {
    render(<Harness loading />);
    expect(screen.getByLabelText("正在读取报文规则能力")).toBeVisible();
  });

  it("shows a capability error and retries", async () => {
    const user = userEvent.setup();
    const reload = vi.fn();
    render(<Harness error="能力目录错误" onReload={reload} />);
    expect(screen.getByText("能力目录错误")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(reload).toHaveBeenCalledOnce();
  });

  it("does not render any HTTP-only control in its DOM or accessibility tree", () => {
    const { container } = render(<Harness />);
    const forbidden = /Method|Path|Query|Header|Cookie|Status Code|JSONPath|HTTP Body|nth[- ]hit|TLS handshake/i;
    expect(container.textContent).not.toMatch(forbidden);
    for (const element of container.querySelectorAll("[aria-label],[aria-description]")) {
      expect(`${element.getAttribute("aria-label") ?? ""} ${element.getAttribute("aria-description") ?? ""}`).not.toMatch(forbidden);
    }
  });

  it("keeps an existing entry, package, schema, and stage binding read-only", () => {
    const relay = listener("relay");
    const draft = { ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()), rule_id: "rule-1", expected_revision: 4 };
    render(<Harness activeListener={relay} creating={false} initialDraft={draft} />);
    expect(screen.getByLabelText("固定规则绑定")).toHaveTextContent("交易中继");
    expect(screen.getByLabelText("固定规则绑定")).toHaveTextContent("iso8583@1.2.3");
    expect(screen.queryByLabelText("协议入口")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("报文处理阶段")).not.toBeInTheDocument();
  });

  it("offers all four processing stages while creating a relay rule", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByLabelText("报文处理阶段"));
    expect(await screen.findByRole("option", { name: "应用 → 代理" })).toBeVisible();
    expect(screen.getByRole("option", { name: "代理 → 上游服务" })).toBeVisible();
    expect(screen.getByRole("option", { name: "上游服务 → 代理" })).toBeVisible();
    expect(screen.getByRole("option", { name: "代理 → 应用" })).toBeVisible();
  });

  it("offers the two app-facing stages for local response", async () => {
    const user = userEvent.setup();
    const local = listener("local", true);
    render(<Harness activeCatalog={catalog("proxy_to_app")} activeListener={local} />);
    await user.click(screen.getByLabelText("报文处理阶段"));
    expect(await screen.findByRole("option", { name: "应用 → 代理" })).toBeVisible();
    expect(screen.getByRole("option", { name: "代理 → 应用" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "代理 → 上游服务" })).not.toBeInTheDocument();
  });

  it("maps binding, condition, action, and general field errors without duplicate general alerts", () => {
    render(<Harness fieldErrors={{
      listener_id: ["入口已变化"],
      "conditions[0]": ["条件非法"],
      actions: ["动作非法"],
      general: ["revision conflict"],
    }} />);
    expect(screen.getByText("入口已变化")).toBeVisible();
    expect(screen.getByText("条件非法")).toBeVisible();
    expect(screen.getByText("动作非法")).toBeVisible();
    expect(screen.getByText("revision conflict")).toBeVisible();
    expect(screen.getAllByRole("alert")).toHaveLength(4);
  });

  it("blocks save while pending or while a typed value is invalid", () => {
    const { unmount } = render(<Harness pending />);
    expect(screen.getByRole("button", { name: "正在保存…" })).toBeDisabled();
    unmount();
    render(<Harness invalidInitially />);
    expect(screen.getByRole("button", { name: "保存报文规则" })).toBeDisabled();
  });

  it("blocks save while Rust is parsing an edited action value", () => {
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise(() => undefined));
    const relay = listener("relay");
    const draft = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [{ type: "set_field" as const, field: "message_type", value: { type: "string" as const, value: "0200" } }],
    };
    const save = vi.fn();
    render(<Harness initialDraft={draft} onSave={save} />);
    fireEvent.change(screen.getByRole("textbox", { name: "设置值" }), { target: { value: "0210" } });
    expect(screen.getByLabelText("正在解析设置值")).toBeVisible();
    expect(screen.getByRole("button", { name: "保存报文规则" })).toBeDisabled();
    expect(save).not.toHaveBeenCalled();
  });

  it("locks other draft paths until a deferred parser result is applied", async () => {
    let finish!: (value: unknown) => void;
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const relay = listener("relay");
    const draft = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [
        { type: "set_field" as const, field: "message_type", value: { type: "string" as const, value: "0200" } },
        { type: "set_field" as const, field: "amount", value: { type: "int" as const, value: 100 } },
      ],
    };
    const save = vi.fn();
    render(<Harness initialDraft={draft} onSave={save} />);
    const values = screen.getAllByRole("textbox", { name: "设置值" });
    fireEvent.change(values[0], { target: { value: "0210" } });
    expect(values[0]).toBeEnabled();
    expect(values[1]).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "优先级" })).toBeDisabled();
    finish({ type: "string", value: "0210" });
    await waitFor(() => expect(screen.queryByLabelText("正在解析设置值")).not.toBeInTheDocument());
    const saveButton = screen.getByRole("button", { name: "保存报文规则" });
    await waitFor(() => expect(saveButton).toBeEnabled());
    await userEvent.setup().click(saveButton);
    expect(save.mock.calls[0][0].actions[1].value).toEqual({ type: "int", value: 100 });
  });

  it("renders stale condition and action fields fail-closed", () => {
    const relay = listener("relay");
    const draft = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      conditions: [{ operator: "equals" as const, field: "removed", value: { type: "string" as const, value: "x" } }],
      actions: [{ type: "set_field" as const, field: "removed", value: { type: "string" as const, value: "x" } }],
    };
    render(<Harness initialDraft={draft} />);
    expect(screen.getByText("条件 1 引用了未知字段。")).toBeVisible();
    expect(screen.getByText("动作 1 引用了不可修改字段。")).toBeVisible();
  });

  it("disables additions at the 64-condition and 64-action limits", () => {
    const relay = listener("relay");
    const draft = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      conditions: Array.from({ length: 64 }, () => ({ operator: "equals" as const, field: "removed", value: { type: "string" as const, value: "x" } })),
      actions: Array.from({ length: 64 }, () => ({ type: "record_match" as const })),
    };
    render(<Harness initialDraft={draft} />);
    expect(screen.getByRole("button", { name: "添加条件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加：记录命中" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加：清空全部字段" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加：设置字段" })).toBeDisabled();
  });

  it("shows only RecordMatch when the capability catalog exposes no mutation actions", () => {
    const recordOnly = {
      ...catalog(),
      fields: fields.map((field) => ({ ...field, actions: [] })),
      common_actions: ["record_match" as const],
    };
    render(<Harness activeCatalog={recordOnly} />);
    expect(screen.getByRole("button", { name: "添加：记录命中" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "添加：设置字段" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "添加：清空全部字段" })).not.toBeInTheDocument();
  });

  it("explains that an empty Schema still supports an unconditional RecordMatch", () => {
    render(<Harness activeCatalog={{ ...catalog(), fields: [], common_actions: ["record_match"] }} />);
    expect(screen.getByText("协议没有可配置字段")).toBeVisible();
    expect(screen.getByText("空条件恒匹配。")).toBeVisible();
  });

  it("adds AND conditions without offering a duplicate field", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    await user.click(screen.getByRole("button", { name: "添加条件" }));
    expect(screen.getAllByLabelText("条件字段")).toHaveLength(2);
    await user.click(screen.getAllByLabelText("条件字段")[1]);
    expect(screen.queryByRole("option", { name: /message_type/ })).not.toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /amount/ })).toBeVisible();
  });

  it("deletes one condition without disturbing the remaining condition", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const initial = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      conditions: [
        { operator: "equals" as const, field: "message_type", value: { type: "string" as const, value: "0200" } },
        { operator: "equals" as const, field: "amount", value: { type: "int" as const, value: 100 } },
      ],
    };
    render(<Harness initialDraft={initial} />);
    await user.click(screen.getByRole("button", { name: "删除条件 1" }));
    expect(screen.getAllByLabelText("条件字段")).toHaveLength(1);
    expect(screen.getByRole("textbox", { name: "比较值" })).toHaveValue("100");
  });

  it("appends RecordMatch without replacing existing actions", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const saved = vi.fn();
    render(<Harness initialDraft={{ ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()), actions: [{ type: "clear_document" }] }} onSave={saved} />);
    await user.click(screen.getByRole("button", { name: "添加：记录命中" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(saved.mock.calls[0][0].actions).toEqual([{ type: "clear_document" }, { type: "record_match" }]);
  });

  it("preserves action order when ClearDocument is followed by SetField", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const initial = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [{ type: "clear_document" as const }],
    };
    const saved = vi.fn();
    render(<Harness initialDraft={initial} onSave={saved} />);
    await user.click(screen.getByRole("button", { name: "添加：设置字段" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(saved).toHaveBeenCalledWith(expect.objectContaining({
      actions: [
        { type: "clear_document" },
        { type: "set_field", field: "message_type", value: { type: "string", value: "" } },
      ],
    }));
  });

  it("moves actions up and down without changing their contents", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const initial = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [{ type: "record_match" as const }, { type: "clear_document" as const }],
    };
    const saved = vi.fn();
    render(<Harness initialDraft={initial} onSave={saved} />);
    await user.click(screen.getByRole("button", { name: "动作 2 上移" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(saved.mock.calls[0][0].actions).toEqual([{ type: "clear_document" }, { type: "record_match" }]);
  });

  it("moves the first action down without changing its contents", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const initial = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [{ type: "record_match" as const }, { type: "clear_document" as const }],
    };
    const saved = vi.fn();
    render(<Harness initialDraft={initial} onSave={saved} />);
    await user.click(screen.getByRole("button", { name: "动作 1 下移" }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(saved.mock.calls[0][0].actions).toEqual([{ type: "clear_document" }, { type: "record_match" }]);
  });

  it("resets a SetField value when selecting another Schema field", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const initial = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      actions: [{ type: "set_field" as const, field: "message_type", value: { type: "string" as const, value: "0210" } }],
    };
    const saved = vi.fn();
    render(<Harness initialDraft={initial} onSave={saved} />);
    await user.click(screen.getByLabelText("设置字段"));
    await user.click(await screen.findByRole("option", { name: /amount/ }));
    await user.click(screen.getByRole("button", { name: "保存报文规则" }));
    expect(saved.mock.calls[0][0].actions).toEqual([{ type: "set_field", field: "amount", value: { type: "int", value: 0 } }]);
  });

  it("does not allow the final action to be deleted", () => {
    render(<Harness />);
    expect(screen.getByRole("button", { name: "删除动作 1" })).toBeDisabled();
  });

  it("requires explicit confirmation before deleting a saved rule", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const draft = { ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()), rule_id: "rule-1", expected_revision: 2 };
    const remove = vi.fn();
    render(<Harness activeListener={relay} creating={false} initialDraft={draft} onDelete={remove} />);
    await user.click(screen.getByRole("button", { name: "删除规则" }));
    expect(screen.getByRole("alertdialog", { name: "删除此报文规则？" })).toBeVisible();
    expect(remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "确认删除" }));
    expect(remove).toHaveBeenCalledOnce();
  });

  it("does not open deletion confirmation while another mutation is pending", async () => {
    const user = userEvent.setup();
    const relay = listener("relay");
    const draft = { ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()), rule_id: "rule-1", expected_revision: 2 };
    render(<Harness activeListener={relay} creating={false} initialDraft={draft} pending />);
    const remove = screen.getByRole("button", { name: "删除规则" });
    expect(remove).toBeDisabled();
    await user.click(remove);
    expect(screen.queryByRole("alertdialog", { name: "删除此报文规则？" })).not.toBeInTheDocument();
  });

  it("locks every draft and context control while a mutation is pending", () => {
    const relay = listener("relay");
    const draft = {
      ...newProtocolRuleDraft(relay, "app_to_proxy", catalog()),
      conditions: [{ operator: "equals" as const, field: "message_type", value: { type: "string" as const, value: "0200" } }],
      actions: [{ type: "record_match" as const }, { type: "set_field" as const, field: "message_type", value: { type: "string" as const, value: "0210" } }],
    };
    render(<Harness activeListener={relay} initialDraft={draft} pending />);
    for (const name of ["协议入口", "报文处理阶段", "启用报文规则", "条件字段", "设置字段"]) {
      expect(screen.getByLabelText(name)).toBeDisabled();
    }
    for (const name of ["优先级", "比较值", "设置值"]) expect(screen.getByRole("textbox", { name })).toBeDisabled();
    for (const name of ["添加条件", "添加：记录命中", "添加：清空全部字段", "添加：设置字段", "删除条件 1", "动作 1 下移", "动作 2 上移", "删除动作 2"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
  });
});
