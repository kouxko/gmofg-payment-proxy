// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentValue, ProtocolRuleFieldCapability } from "@/generated/rust-types";
import { ProtocolRuleValueEditor } from "./socket-rule-value-editor";

const commandMocks = vi.hoisted(() => ({ protocolRuleParseValue: vi.fn() }));
vi.mock("@/generated/rust-types", () => ({ commands: commandMocks }));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : "Rust 解析失败",
}));

const fields: ProtocolRuleFieldCapability[] = [
  { name: "message_type", label: "消息类型", type: "string", operators: ["equals"], actions: ["set_field"] },
  { name: "amount", label: "金额", type: "int", operators: ["equals"], actions: ["set_field"] },
  { name: "approved", label: "批准", type: "bool", operators: ["equals"], actions: ["set_field"] },
  { name: "bitmap", label: "位图", type: "blob", operators: ["equals"], actions: ["set_field"] },
];

beforeEach(() => {
  commandMocks.protocolRuleParseValue.mockImplementation(async (type: string, raw: string) => {
    if (type === "string") return { type, value: raw };
    if (type === "int") return { type, value: Number(raw) };
    if (type === "bool") return { type, value: raw === "true" };
    const compact = raw.replace(/\s/g, "");
    return { type, value: compact.match(/.{2}/g)?.map((byte) => Number.parseInt(byte, 16)) ?? [] };
  });
});

describe("Socket typed value editor Rust parsing", () => {
  it.each([
    [fields[0], { type: "string", value: "初始" }, "报文0210", { type: "string", value: "报文0210" }],
    [fields[1], { type: "int", value: 1 }, "9007199254740991", { type: "int", value: Number.MAX_SAFE_INTEGER }],
    [fields[3], { type: "blob", value: [1] }, "02 a0 ff", { type: "blob", value: [2, 160, 255] }],
  ] as const)("calls Rust and emits a typed $name payload", async (field, initial, text, expected) => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<ProtocolRuleValueEditor field={field} label="值" value={initial as DocumentValue} onChange={change} onAsyncStateChange={vi.fn()} />);
    const input = screen.getByRole("textbox", { name: "值" });
    await user.clear(input);
    await user.type(input, text);
    await waitFor(() => expect(change).toHaveBeenLastCalledWith(expected));
    expect(commandMocks.protocolRuleParseValue).toHaveBeenLastCalledWith(field.type, text);
  });

  it("calls Rust for an explicit boolean payload", async () => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(<ProtocolRuleValueEditor field={fields[2]} label="值" value={{ type: "bool", value: false }} onChange={change} onAsyncStateChange={vi.fn()} />);
    await user.click(screen.getByLabelText("值"));
    await user.click(await screen.findByRole("option", { name: "true" }));
    await waitFor(() => expect(change).toHaveBeenCalledWith({ type: "bool", value: true }));
    expect(commandMocks.protocolRuleParseValue).toHaveBeenCalledWith("bool", "true");
  });

  it("shows a Rust field error and marks the value invalid", async () => {
    commandMocks.protocolRuleParseValue.mockRejectedValueOnce(new Error("整数超出 Rust 安全范围"));
    const change = vi.fn();
    const asyncState = vi.fn();
    render(<ProtocolRuleValueEditor field={fields[1]} label="值" value={{ type: "int", value: 1 }} onChange={change} onAsyncStateChange={asyncState} />);
    fireEvent.change(screen.getByRole("textbox", { name: "值" }), { target: { value: "9007199254740992" } });
    expect(await screen.findByText("整数超出 Rust 安全范围")).toBeVisible();
    expect(asyncState).toHaveBeenLastCalledWith({ pending: false, invalid: true });
    expect(change).not.toHaveBeenCalled();
  });

  it.each([
    [{ type: "string", value: "100" }, "Rust 返回了错误的字段类型"],
    [{ unexpected: true }, "Rust 返回了无效的字段值"],
  ])("rejects a malformed Rust parse payload", async (payload, expectedError) => {
    commandMocks.protocolRuleParseValue.mockResolvedValueOnce(payload);
    const change = vi.fn();
    const asyncState = vi.fn();
    render(<ProtocolRuleValueEditor field={fields[1]} label="值" value={{ type: "int", value: 1 }} onChange={change} onAsyncStateChange={asyncState} />);
    fireEvent.change(screen.getByRole("textbox", { name: "值" }), { target: { value: "100" } });
    expect(await screen.findByText(expectedError)).toBeVisible();
    expect(asyncState).toHaveBeenLastCalledWith({ pending: false, invalid: true });
    expect(change).not.toHaveBeenCalled();
  });

  it("discards an older Rust parse response", async () => {
    let finishFirst!: (value: DocumentValue) => void;
    let finishSecond!: (value: DocumentValue) => void;
    commandMocks.protocolRuleParseValue
      .mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve; }))
      .mockReturnValueOnce(new Promise((resolve) => { finishSecond = resolve; }));
    const change = vi.fn();
    render(<ProtocolRuleValueEditor field={fields[0]} label="值" value={{ type: "string", value: "" }} onChange={change} onAsyncStateChange={vi.fn()} />);
    const input = screen.getByRole("textbox", { name: "值" });
    fireEvent.change(input, { target: { value: "first" } });
    fireEvent.change(input, { target: { value: "second" } });
    await act(async () => { finishSecond({ type: "string", value: "second" }); await Promise.resolve(); });
    expect(change).toHaveBeenLastCalledWith({ type: "string", value: "second" });
    await act(async () => { finishFirst({ type: "string", value: "first" }); await Promise.resolve(); });
    expect(change).toHaveBeenCalledTimes(1);
  });

  it("reports pending then clears state when unmounted", () => {
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise(() => undefined));
    const asyncState = vi.fn();
    const { unmount } = render(<ProtocolRuleValueEditor field={fields[0]} label="值" value={{ type: "string", value: "" }} onChange={vi.fn()} onAsyncStateChange={asyncState} />);
    fireEvent.change(screen.getByRole("textbox", { name: "值" }), { target: { value: "pending" } });
    expect(screen.getByLabelText("正在解析值")).toBeVisible();
    expect(asyncState).toHaveBeenLastCalledWith({ pending: true, invalid: false });
    unmount();
    expect(asyncState).toHaveBeenLastCalledWith(undefined);
  });

  it("cancels a pending parse for a reloaded value or field", async () => {
    commandMocks.protocolRuleParseValue.mockReturnValue(new Promise(() => undefined));
    const asyncState = vi.fn();
    const props = { label: "值", onChange: vi.fn(), onAsyncStateChange: asyncState };
    const { rerender } = render(<ProtocolRuleValueEditor {...props} field={fields[1]} value={{ type: "int", value: 1 }} />);
    const input = screen.getByRole("textbox", { name: "值" });
    fireEvent.change(input, { target: { value: "-" } });
    expect(input).toHaveValue("-");
    rerender(<ProtocolRuleValueEditor {...props} field={fields[1]} value={{ type: "int", value: 2 }} />);
    await waitFor(() => expect(input).toHaveValue("2"));
    expect(asyncState).toHaveBeenLastCalledWith(undefined);
    rerender(<ProtocolRuleValueEditor {...props} field={{ ...fields[1], name: "trace" }} value={{ type: "int", value: 7 }} />);
    await waitFor(() => expect(input).toHaveValue("7"));
  });
});
