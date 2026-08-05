import { useEffect, useRef, useState } from "react";
import {
  FieldError,
  Input,
  Label,
  ListBox,
  Select,
  TextArea,
  TextField,
} from "@heroui/react";
import type { RuleTerminalAction } from "@/generated/rust-types";
import { errorMessage } from "@/lib/ipc/client";
import { NumericInput } from "./rule-editor-controls";
import {
  parseRuleByteInput,
  parseRuleHeaderInput,
  type TerminalActionUpdate,
} from "./rule-editor-model";

type ParseStateChange = (
  field: "bytes" | "headers",
  state?: { pending: boolean; invalid: boolean },
) => void;

export function TerminalActionFields({
  action,
  onChange,
  onAsyncStateChange,
}: {
  action: RuleTerminalAction;
  onChange: (update: TerminalActionUpdate) => void;
  onAsyncStateChange: ParseStateChange;
}) {
  const currentBytes =
    action.type === "mock_response" || action.type === "invalid_json"
      ? action.body_bytes
      : [];
  const [rawBytes, setRawBytes] = useState(currentBytes.join(", "));
  const [byteError, setByteError] = useState<string>();
  const initialHeaders =
    action.type === "mock_response"
      ? action.headers.map(([name, value]) => `${name}: ${value}`).join("\n")
      : "";
  const [rawHeaders, setRawHeaders] = useState(initialHeaders);
  const [headerError, setHeaderError] = useState<string>();
  const byteGeneration = useRef(0);
  const headerGeneration = useRef(0);
  const asyncStateChangeRef = useRef(onAsyncStateChange);

  useEffect(() => {
    asyncStateChangeRef.current = onAsyncStateChange;
  }, [onAsyncStateChange]);
  useEffect(
    () => () => {
      byteGeneration.current += 1;
      headerGeneration.current += 1;
      asyncStateChangeRef.current("bytes", undefined);
      asyncStateChangeRef.current("headers", undefined);
    },
    [],
  );

  async function commitRawBytes(raw: string) {
    if (action.type !== "mock_response" && action.type !== "invalid_json")
      return;
    const generation = ++byteGeneration.current;
    onAsyncStateChange("bytes", { pending: true, invalid: false });
    try {
      const parsed = await parseRuleByteInput(raw);
      if (generation !== byteGeneration.current) return;
      setRawBytes(parsed.normalized);
      setByteError(undefined);
      onChange((current) =>
        current.type === "mock_response" || current.type === "invalid_json"
          ? { ...current, body_bytes: parsed.bytes }
          : current,
      );
      onAsyncStateChange("bytes", undefined);
    } catch (reason) {
      if (generation !== byteGeneration.current) return;
      setByteError(errorMessage(reason));
      onAsyncStateChange("bytes", { pending: false, invalid: true });
    }
  }

  async function commitRawHeaders(raw: string) {
    if (action.type !== "mock_response") return;
    const generation = ++headerGeneration.current;
    onAsyncStateChange("headers", { pending: true, invalid: false });
    try {
      const parsed = await parseRuleHeaderInput(raw);
      if (generation !== headerGeneration.current) return;
      setRawHeaders(parsed.normalized);
      setHeaderError(undefined);
      onChange((current) =>
        current.type === "mock_response"
          ? { ...current, headers: parsed.headers }
          : current,
      );
      onAsyncStateChange("headers", undefined);
    } catch (reason) {
      if (generation !== headerGeneration.current) return;
      setHeaderError(errorMessage(reason));
      onAsyncStateChange("headers", { pending: false, invalid: true });
    }
  }

  switch (action.type) {
    case "upstream_connect_timeout":
    case "upstream_write_timeout":
    case "upstream_read_timeout":
      return (
        <NumericInput
          label="超时（毫秒）"
          value={action.milliseconds}
          onChange={(milliseconds) =>
            onChange((current) =>
              current.type === action.type
                ? { ...current, milliseconds }
                : current,
            )
          }
        />
      );
    case "drop_upstream_response":
      return (
        <div className="grid gap-1">
          <Label>丢弃模式</Label>
          <Select
            aria-label="丢弃模式"
            selectedKey={action.mode}
            onSelectionChange={(mode) =>
              onChange((current) =>
                current.type === "drop_upstream_response"
                  ? { ...current, mode: mode as typeof action.mode }
                  : current,
              )
            }
          >
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                <ListBox.Item id="read_complete_response">
                  读完响应后关闭
                </ListBox.Item>
                <ListBox.Item id="close_after_request_write">
                  写完请求后关闭
                </ListBox.Item>
              </ListBox>
            </Select.Popover>
          </Select>
        </div>
      );
    case "mock_response":
      return (
        <div className="grid gap-3">
          <NumericInput
            label="HTTP 状态码"
            value={action.status}
            onChange={(status) =>
              onChange((current) =>
                current.type === "mock_response"
                  ? { ...current, status }
                  : current,
              )
            }
          />
          <TextField isInvalid={headerError != null}>
            <Label>响应 Header（每行 name: value）</Label>
            <TextArea
              aria-label="响应 Header（每行 name: value）"
              value={rawHeaders}
              onChange={(event) => {
                const raw = event.target.value;
                setRawHeaders(raw);
                setHeaderError(undefined);
                void commitRawHeaders(raw);
              }}
            />
            {headerError && <FieldError>{headerError}</FieldError>}
          </TextField>
          <ByteInput
            raw={rawBytes}
            error={byteError}
            onChange={(raw) => {
              setRawBytes(raw);
              setByteError(undefined);
              void commitRawBytes(raw);
            }}
          />
        </div>
      );
    case "invalid_json":
      return (
        <ByteInput
          raw={rawBytes}
          error={byteError}
          onChange={(raw) => {
            setRawBytes(raw);
            setByteError(undefined);
            void commitRawBytes(raw);
          }}
        />
      );
    case "incorrect_content_length":
      return (
        <NumericInput
          label="长度差值"
          value={action.delta}
          onChange={(delta) =>
            onChange((current) =>
              current.type === "incorrect_content_length"
                ? { ...current, delta }
                : current,
            )
          }
        />
      );
    case "truncate_response":
      return (
        <NumericInput
          label="截断字节数"
          value={action.bytes}
          onChange={(bytes) =>
            onChange((current) =>
              current.type === "truncate_response"
                ? { ...current, bytes }
                : current,
            )
          }
        />
      );
    case "disconnect_during_upstream_write":
    case "disconnect_during_downstream_write":
      return (
        <NumericInput
          label="发送后断连（字节）"
          value={action.after_bytes}
          onChange={(after_bytes) =>
            onChange((current) =>
              current.type === action.type
                ? { ...current, after_bytes }
                : current,
            )
          }
        />
      );
    case "reject_tls_handshake":
    case "disconnect_before_upstream":
      return null;
  }
}

function ByteInput({
  raw,
  error,
  onChange,
}: {
  raw: string;
  error?: string;
  onChange: (raw: string) => void;
}) {
  return (
    <TextField isInvalid={error != null}>
      <Label>Shift-JIS Body 字节</Label>
      <Input value={raw} onChange={(event) => onChange(event.target.value)} />
      {error && <FieldError>{error}</FieldError>}
    </TextField>
  );
}
