// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  ProxyWorkspace,
  ResponseAssertionKind,
} from "@/generated/rust-types";
import type { ComponentKind } from "./workspace-components-editor-model";
import { WorkspaceComponentsEditor } from "./workspace-components-editor";

const workspace = {
  id: "workspace-1",
  name: "API Lab",
  revision: 3,
  listeners: [],
  response_assertions: [
    {
      id: "assertion-1",
      name: "success body",
      listener_ids: ["listener-a"],
      enabled: true,
      assertion: { kind: "body_text_contains", expected: "approved" },
    },
  ],
  certificate_references: [
    {
      id: "certificate-1",
      label: "Listener identity",
      kind: "reverse_server_identity",
      reference: "managed:listener-tls:listener-a",
    },
    {
      id: "certificate-2",
      label: "Upstream trust",
      kind: "upstream_server_trust",
      reference: "/external/upstream-ca.pem",
    },
  ],
  fault_presets: [
    {
      id: "fault-1",
      name: "slow connection",
      description: "Delay connection establishment",
      connection_actions: [{ kind: "delay", milliseconds: 200 }],
    },
  ],
} as unknown as ProxyWorkspace;

type EditorSpies = {
  onChange: ReturnType<typeof vi.fn<(next: ProxyWorkspace) => void>>;
  onAdd: ReturnType<typeof vi.fn<(kind: ComponentKind) => void>>;
  onIntent: ReturnType<
    typeof vi.fn<(kind: ComponentKind, id: string, operation: string, value: string) => void>
  >;
};

function renderEditor({
  disabled = false,
  currentWorkspace = workspace,
}: {
  disabled?: boolean;
  currentWorkspace?: ProxyWorkspace;
} = {}): EditorSpies {
  const spies: EditorSpies = {
    onChange: vi.fn(),
    onAdd: vi.fn(),
    onIntent: vi.fn(),
  };

  render(
    <WorkspaceComponentsEditor
      workspace={currentWorkspace}
      onChange={spies.onChange}
      onAdd={spies.onAdd}
      onIntent={spies.onIntent}
      disabled={disabled}
    />,
  );

  return spies;
}

function workspaceWithAssertion(assertion: ResponseAssertionKind): ProxyWorkspace {
  return {
    ...workspace,
    response_assertions: [{ ...workspace.response_assertions[0], assertion }],
  };
}

async function openTab(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole("tab", { name }));
}

describe("WorkspaceComponentsEditor", () => {
  it("only exposes the three supported Workspace strategy tabs", () => {
    renderEditor();

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "响应断言",
      "证书引用",
      "连接故障预设",
    ]);
    expect(screen.queryByText("元数据提取")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "新增提取器" })).not.toBeInTheDocument();
  });

  it.each([
    ["响应断言", "新增响应断言", "response_assertion"],
    ["连接故障预设", "新增连接故障预设", "fault_preset"],
  ] as const)("adds a component from the %s tab", async (tab, button, kind) => {
    const user = userEvent.setup();
    const { onAdd } = renderEditor();

    await openTab(user, tab);
    await user.click(screen.getByRole("button", { name: button }));

    expect(onAdd).toHaveBeenCalledOnce();
    expect(onAdd).toHaveBeenCalledWith(kind);
  });

  it.each([
    ["响应断言", "删除响应断言 1", "response_assertion", "assertion-1"],
    ["证书引用", "删除证书引用 1", "certificate_reference", "certificate-1"],
    ["连接故障预设", "删除故障预设 1", "fault_preset", "fault-1"],
  ] as const)("deletes a component from the %s tab", async (tab, button, kind, id) => {
    const user = userEvent.setup();
    const { onIntent } = renderEditor();

    await openTab(user, tab);
    await user.click(screen.getByRole("button", { name: button }));

    expect(onIntent).toHaveBeenCalledOnce();
    expect(onIntent).toHaveBeenCalledWith(kind, id, "delete", "");
  });

  it.each([
    ["响应断言", "响应断言 1 类型", "Header 等于", "response_assertion", "assertion-1", "header_equals"],
    ["连接故障预设", "故障预设 1 动作", "拒绝连接", "fault_preset", "fault-1", "reject"],
  ] as const)(
    "delegates a variant change from the %s tab",
    async (tab, select, option, kind, id, variant) => {
      const user = userEvent.setup();
      const { onIntent } = renderEditor();

      await openTab(user, tab);
      await user.click(screen.getByLabelText(select));
      await user.click(await screen.findByRole("option", { name: option }));

      expect(onIntent).toHaveBeenCalledOnce();
      expect(onIntent).toHaveBeenCalledWith(kind, id, "variant", variant);
    },
  );

  it("maps an edited assertion value back to the selected assertion", async () => {
    const user = userEvent.setup();
    const { onChange } = renderEditor();

    await openTab(user, "响应断言");
    fireEvent.change(screen.getByDisplayValue("approved"), {
      target: { value: "accepted" },
    });

    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange.mock.calls[0][0].response_assertions[0].assertion).toEqual({
      kind: "body_text_contains",
      expected: "accepted",
    });
  });

  it.each([
    [
      { kind: "header_equals", name: "X-Mode", expected: "ready" },
      "ready",
      "done",
      { kind: "header_equals", name: "X-Mode", expected: "done" },
    ],
    [
      { kind: "json_path_equals", path: "$.status", expected: "ready" },
      "ready",
      "done",
      { kind: "json_path_equals", path: "$.status", expected: "done" },
    ],
    [
      { kind: "body_sha256_equals", expected_hex: "abcd" },
      "abcd",
      "ef01",
      { kind: "body_sha256_equals", expected_hex: "ef01" },
    ],
  ] as const)("maps the %s text assertion variant", async (assertion, current, next, expected) => {
    const user = userEvent.setup();
    const { onChange } = renderEditor({
      currentWorkspace: workspaceWithAssertion(assertion),
    });

    await openTab(user, "响应断言");
    fireEvent.change(screen.getByDisplayValue(current), { target: { value: next } });

    expect(onChange.mock.calls[0][0].response_assertions[0].assertion).toEqual(expected);
  });

  it.each([
    [
      { kind: "http_status_equals", expected: 200 },
      "Increase 期望状态码",
      { kind: "http_status_equals", expected: 201 },
    ],
    [
      { kind: "body_length_equals", expected: 4 },
      "Increase 期望字节数",
      { kind: "body_length_equals", expected: 5 },
    ],
  ] as const)("maps the %s numeric assertion variant", async (assertion, control, expected) => {
    const user = userEvent.setup();
    const { onChange } = renderEditor({
      currentWorkspace: workspaceWithAssertion(assertion),
    });

    await openTab(user, "响应断言");
    await user.click(screen.getByRole("button", { name: control }));

    expect(onChange.mock.calls[0][0].response_assertions[0].assertion).toEqual(expected);
  });

  it("maps the assertion enabled switch back to the assertion", async () => {
    const user = userEvent.setup();
    const { onChange } = renderEditor();

    await openTab(user, "响应断言");
    await user.click(screen.getByRole("switch", { name: "启用" }));

    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange.mock.calls[0][0].response_assertions[0].enabled).toBe(false);
  });

  it("maps an incremented fault value back to the selected connection action", async () => {
    const user = userEvent.setup();
    const { onChange } = renderEditor();

    await openTab(user, "连接故障预设");
    await user.click(screen.getByRole("button", { name: "Increase 毫秒" }));

    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange.mock.calls.at(-1)?.[0].fault_presets[0].connection_actions).toEqual([
      { kind: "delay", milliseconds: 201 },
    ]);
  });

  it("shows certificate kind and storage mappings in the certificate tab", async () => {
    const user = userEvent.setup();
    renderEditor();

    await openTab(user, "证书引用");

    expect(screen.getByText("Reverse 服务端身份")).toBeVisible();
    expect(screen.getByText("系统密钥保护的 Listener TLS 引用")).toBeVisible();
    expect(screen.getByText("上游服务端信任")).toBeVisible();
    expect(screen.getByText("外部文件引用（建议在入口配置中重新导入）")).toBeVisible();
  });

  it.each([
    ["响应断言", "listener-a", "response_assertion", "assertion-1"],
  ] as const)(
    "submits listener IDs only when the %s input loses focus",
    async (tab, currentValue, kind, id) => {
      const user = userEvent.setup();
      const { onIntent } = renderEditor();

      await openTab(user, tab);
      const input = screen.getByDisplayValue(currentValue);
      fireEvent.change(input, { target: { value: "listener-c, listener-d" } });
      expect(onIntent).not.toHaveBeenCalled();

      fireEvent.blur(input);
      expect(onIntent).toHaveBeenCalledOnce();
      expect(onIntent).toHaveBeenCalledWith(
        kind,
        id,
        "listener_ids",
        "listener-c, listener-d",
      );
    },
  );

  it.each([
    ["响应断言", "新增响应断言"],
    ["连接故障预设", "新增连接故障预设"],
  ] as const)("disables adding from the %s tab while busy", async (tab, button) => {
    const user = userEvent.setup();
    renderEditor({ disabled: true });

    await openTab(user, tab);

    expect(screen.getByRole("button", { name: button })).toBeDisabled();
  });

  it("disables every draft mutation while an IPC action is pending", async () => {
    const user = userEvent.setup();
    renderEditor({ disabled: true });

    expect(screen.getByRole("switch", { name: "启用" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除响应断言 1" })).toBeDisabled();

    await openTab(user, "证书引用");
    expect(screen.getByRole("button", { name: "删除证书引用 1" })).toBeDisabled();

    await openTab(user, "连接故障预设");
    expect(screen.getByDisplayValue("slow connection")).toBeDisabled();
    expect(screen.getByLabelText("故障预设 1 动作")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Increase 毫秒" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除故障预设 1" })).toBeDisabled();
  });
});
