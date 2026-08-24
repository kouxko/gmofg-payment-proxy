import { Input, Label, ListBox, Select, TextArea } from "@heroui/react";
import type { RuleAction } from "@/generated/rust-types";
import { NumericInput } from "./rule-editor-controls";
import type { ActionUpdate } from "./rule-editor-model";
import { TerminalActionFields } from "./terminal-action-fields";

export function ActionFields({
  action,
  onChange,
  onAsyncStateChange,
  asyncStateKey,
  trafficDirection,
}: {
  action: RuleAction;
  onChange: (update: ActionUpdate) => void;
  onAsyncStateChange: (
    key: string,
    state?: { pending: boolean; invalid: boolean },
  ) => void;
  asyncStateKey: string;
  trafficDirection?: "upstream" | "downstream";
}) {
  switch (action.type) {
    case "set_json_field":
      return (
        <>
          <Input
            aria-label="JSON Path"
            value={action.path}
            onChange={(event) =>
              onChange((current) =>
                current.type === "set_json_field"
                  ? { ...current, path: event.target.value }
                  : current,
              )
            }
          />
          <TextArea
            aria-label="JSON 值"
            value={action.value_json}
            onChange={(event) =>
              onChange((current) =>
                current.type === "set_json_field"
                  ? { ...current, value_json: event.target.value }
                  : current,
              )
            }
          />
        </>
      );
    case "replace_body_text":
      return (
        <TextArea
          aria-label="Body 文本"
          value={action.text}
          onChange={(event) =>
            onChange((current) =>
              current.type === "replace_body_text"
                ? { ...current, text: event.target.value }
                : current,
            )
          }
        />
      );
    case "set_header":
      return (
        <>
          <Input
            aria-label="Header 名称"
            value={action.name}
            onChange={(event) =>
              onChange((current) =>
                current.type === "set_header"
                  ? { ...current, name: event.target.value }
                  : current,
              )
            }
          />
          <Input
            aria-label="Header 值"
            value={action.value}
            onChange={(event) =>
              onChange((current) =>
                current.type === "set_header"
                  ? { ...current, value: event.target.value }
                  : current,
              )
            }
          />
        </>
      );
    case "delay":
      return (
        <NumericInput
          label="延迟（毫秒）"
          value={action.milliseconds}
          onChange={(milliseconds) =>
            onChange((current) =>
              current.type === "delay" ? { ...current, milliseconds } : current,
            )
          }
        />
      );
    case "jitter":
      return (
        <div className="grid gap-3">
          <div className="grid grid-cols-2 gap-3">
            <NumericInput
              label="最小抖动（毫秒）"
              value={action.minimum_milliseconds}
              onChange={(minimum_milliseconds) =>
                onChange((current) =>
                  current.type === "jitter"
                    ? { ...current, minimum_milliseconds }
                    : current,
                )
              }
            />
            <NumericInput
              label="最大抖动（毫秒）"
              value={action.maximum_milliseconds}
              onChange={(maximum_milliseconds) =>
                onChange((current) =>
                  current.type === "jitter"
                    ? { ...current, maximum_milliseconds }
                    : current,
                )
              }
            />
          </div>
          <div className="grid gap-1">
            <Label>抖动范围</Label>
            <Select
              aria-label="抖动范围"
              selectedKey={action.scope}
              onSelectionChange={(scope) =>
                onChange((current) =>
                  current.type === "jitter"
                    ? { ...current, scope: scope as typeof action.scope }
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
                  <ListBox.Item id="before_message">
                    消息发送前一次
                  </ListBox.Item>
                  <ListBox.Item id="per_chunk">每个分块</ListBox.Item>
                </ListBox>
              </Select.Popover>
            </Select>
          </div>
        </div>
      );
    case "throttle":
      return (
        <div className="grid gap-3">
          <div className="grid grid-cols-2 gap-3">
            <NumericInput
              label="速率（B/s）"
              value={action.bytes_per_second}
              onChange={(bytes_per_second) =>
                onChange((current) =>
                  current.type === "throttle"
                    ? { ...current, bytes_per_second }
                    : current,
                )
              }
            />
            <NumericInput
              label="分块大小（字节）"
              value={action.chunk_bytes}
              onChange={(chunk_bytes) =>
                onChange((current) =>
                  current.type === "throttle"
                    ? { ...current, chunk_bytes }
                    : current,
                )
              }
            />
          </div>
          <TrafficDirection direction={trafficDirection} />
        </div>
      );
    case "intermittent":
      return (
        <div className="grid gap-3">
          <div className="grid grid-cols-2 gap-3">
            <NumericInput
              label="可用窗口（毫秒）"
              value={action.available_milliseconds}
              onChange={(available_milliseconds) =>
                onChange((current) =>
                  current.type === "intermittent"
                    ? { ...current, available_milliseconds }
                    : current,
                )
              }
            />
            <NumericInput
              label="阻断窗口（毫秒）"
              value={action.blocked_milliseconds}
              onChange={(blocked_milliseconds) =>
                onChange((current) =>
                  current.type === "intermittent"
                    ? { ...current, blocked_milliseconds }
                    : current,
                )
              }
            />
          </div>
          <TrafficDirection direction={trafficDirection} />
        </div>
      );
    case "custom_http_status":
      return (
        <NumericInput
          label="HTTP 状态码"
          value={action.status}
          onChange={(status) =>
            onChange((current) =>
              current.type === "custom_http_status"
                ? { ...current, status }
                : current,
            )
          }
        />
      );
    case "terminal":
      return (
        <TerminalActionFields
          action={action.action}
          onChange={(update) =>
            onChange((current) =>
              current.type === "terminal"
                ? { ...current, action: update(current.action) }
                : current,
            )
          }
          onAsyncStateChange={(field, state) =>
            onAsyncStateChange(`${asyncStateKey}:${field}`, state)
          }
        />
      );
    case "pause":
      return (
        <p className="text-sm text-[var(--telemetry-muted)]">
          命中后暂停消息，等待断点工作台处理。
        </p>
      );
  }
}

function TrafficDirection({
  direction,
}: {
  direction?: "upstream" | "downstream";
}) {
  return (
    <p className="text-sm text-[var(--telemetry-muted)]">
      流量方向由阶段固定：
      {direction === "upstream"
        ? "上行 Proxy → Server"
        : direction === "downstream"
          ? "下行 Proxy → App"
          : "当前动作不可用"}
    </p>
  );
}
