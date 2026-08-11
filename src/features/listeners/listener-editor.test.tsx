// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import type { ProxyListener } from "@/generated/rust-types";
import { dynamicListener, fixedListener } from "./listeners-view.test-support";
import { ListenerEditor } from "./listener-editor";

type ListenerEditorProps = ComponentProps<typeof ListenerEditor>;

function editorProps(
  overrides: Partial<ListenerEditorProps> = {},
): ListenerEditorProps {
  return {
    listener: dynamicListener(),
    certificateReferences: [],
    certificateDetails: [],
    basicUsername: "",
    basicPassword: "",
    onBasicUsernameChange: vi.fn(),
    onBasicPasswordChange: vi.fn(),
    onChange: vi.fn(),
    onStoreBasicCredential: vi.fn().mockResolvedValue(undefined),
    onImportDownstreamServerIdentity: vi.fn().mockResolvedValue(true),
    onImportDownstreamClientTrust: vi.fn().mockResolvedValue(true),
    onImportClientIdentity: vi.fn().mockResolvedValue(true),
    onImportServerTrust: vi.fn().mockResolvedValue(true),
    onTestUpstreamTls: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("ListenerEditor", () => {
  it.each([
    ["连接超时毫秒", "connect_timeout_ms", 30001],
    ["读取超时毫秒", "read_timeout_ms", 70001],
    ["写入超时毫秒", "write_timeout_ms", 70001],
  ] as const)("%s 变更只更新对应的 timeout", async (label, field, value) => {
    const props = editorProps();
    const user = userEvent.setup();
    render(<ListenerEditor {...props} />);

    await user.click(screen.getByRole("button", { name: `Increase ${label}` }));

    expect(props.onChange).toHaveBeenLastCalledWith({ [field]: value });
  });

  it.each([
    [["127.0.0.1/32"], "", []],
    [[], " 127.0.0.1/32, 10.0.0.0/8,  ", ["127.0.0.1/32", "10.0.0.0/8"]],
  ] as const)("将 CIDR 输入 %j 规范化为列表", (initial, input, expected) => {
    const props = editorProps({
      listener: { ...dynamicListener(), allowed_client_cidrs: [...initial] },
    });
    render(<ListenerEditor {...props} />);

    fireEvent.change(screen.getByRole("textbox", { name: "允许的客户端 CIDR" }), {
      target: { value: input },
    });

    expect(props.onChange).toHaveBeenLastCalledWith({
      allowed_client_cidrs: expected,
    });
  });

  it("启用 Basic Auth 时创建系统安全凭据引用意图", async () => {
    const props = editorProps();
    const user = userEvent.setup();
    render(<ListenerEditor {...props} />);

    await user.click(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" }));

    expect(props.onChange).toHaveBeenCalledWith({
      authentication: {
        mode: "basic",
        credential: { provider: "system", key: "" },
      },
    });
  });

  it("提交 Basic 凭据时只触发安全存储意图", async () => {
    const props = editorProps({
      listener: {
        ...dynamicListener(),
        authentication: {
          mode: "basic",
          credential: { provider: "system", key: "" },
        },
      },
      basicUsername: "operator",
      basicPassword: "secret",
    });
    const user = userEvent.setup();
    render(<ListenerEditor {...props} />);

    await user.click(screen.getByRole("button", { name: "保护并引用" }));

    expect(props.onStoreBasicCredential).toHaveBeenCalledOnce();
    expect(props.onChange).not.toHaveBeenCalled();
  });

  it("未同时填写 Basic 用户名和密码时禁用安全存储", () => {
    const props = editorProps({
      listener: {
        ...dynamicListener(),
        authentication: {
          mode: "basic",
          credential: { provider: "system", key: "" },
        },
      },
      basicUsername: "operator",
    });
    render(<ListenerEditor {...props} />);

    expect(screen.getByRole("button", { name: "保护并引用" })).toBeDisabled();
  });

  it("将 MITM allowlist 输入规范化为 authority 列表", () => {
    const listener: ProxyListener = dynamicListener();
    listener.mitm.enabled = true;
    const props = editorProps({ listener });
    render(<ListenerEditor {...props} />);

    fireEvent.change(screen.getByRole("textbox", { name: "MITM authority allowlist" }), {
      target: { value: " api.example.test, *.test.example, " },
    });

    expect(props.onChange).toHaveBeenLastCalledWith({
      mitm: {
        ...listener.mitm,
        authority_allowlist: ["api.example.test", "*.test.example"],
      },
    });
  });

  it("MITM 叶子证书缓存变更保留其他 MITM 设置", async () => {
    const listener: ProxyListener = dynamicListener();
    listener.mitm = {
      ...listener.mitm,
      enabled: true,
      authority_allowlist: ["api.example.test"],
    };
    const props = editorProps({ listener });
    const user = userEvent.setup();
    render(<ListenerEditor {...props} />);

    await user.click(screen.getByRole("button", { name: "Decrease MITM 叶子证书缓存" }));

    expect(props.onChange).toHaveBeenLastCalledWith({
      mitm: {
        ...listener.mitm,
        maximum_cached_leaf_certificates: 255,
      },
    });
  });

  it("固定 Server 分支隐藏动态 MITM 并显示上游 TLS 操作", () => {
    const props = editorProps({
      listener: fixedListener("fixed-1", "固定 Server", 9443, "https://server.test:443"),
    });
    render(<ListenerEditor {...props} />);

    expect(screen.queryByRole("switch", { name: "启用 allowlist MITM" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "固定 Server URL" })).toHaveValue("https://server.test:443");
    expect(screen.getByRole("button", { name: "测试上游 TLS / mTLS 握手" })).toBeEnabled();
  });

  it("pending 状态禁用所有异步凭据和 TLS 操作", () => {
    const listener: ProxyListener = fixedListener(
      "fixed-1",
      "固定 Server",
      9443,
      "https://server.test:443",
    );
    listener.authentication = {
      mode: "basic",
      credential: { provider: "system", key: "" },
    };
    const props = editorProps({
      listener,
      basicUsername: "operator",
      basicPassword: "secret",
      pending: "secret",
    });
    render(<ListenerEditor {...props} />);

    expect(screen.getByRole("button", { name: "保护中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "导入 Server CA" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "导入 client.p12" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "测试上游 TLS / mTLS 握手" })).toBeDisabled();
  });
});
