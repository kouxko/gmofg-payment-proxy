import { useCallback, useEffect, useRef } from "react";
import { toast } from "@heroui/react";
import type {
  RuleAction,
  RuleActionKind,
  RuleCondition,
  RuleConditionKind,
  RuleDraft,
  RuleMatchField,
  RuleMatchFieldKind,
  RuleMatchOperator,
  RuleMatchOperatorKind,
  RuleTerminalAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";

export type ConditionKind = RuleConditionKind;
export type ActionKind = RuleActionKind;
export type RuleDraftChange = RuleDraft | ((current: RuleDraft) => RuleDraft);

export type AsyncEditorState = { pending: boolean; invalid: boolean };
export type AsyncStateChange = (key: string, state?: AsyncEditorState) => void;
export type ConditionUpdate = (current: RuleCondition) => RuleCondition;
export type ActionUpdate = (current: RuleAction) => RuleAction;
export type TerminalActionUpdate = (current: RuleTerminalAction) => RuleTerminalAction;

export function useAsyncRequestSlots(prefix: string, onAsyncStateChange: AsyncStateChange) {
  const generations = useRef(new Map<string, number>());
  const activeKeys = useRef(new Set<string>());

  useEffect(() => {
    const currentGenerations = generations.current;
    const currentKeys = activeKeys.current;
    return () => {
      currentKeys.forEach((key) => {
        currentGenerations.set(key, (currentGenerations.get(key) ?? 0) + 1);
        onAsyncStateChange(key, undefined);
      });
      currentKeys.clear();
    };
  }, [onAsyncStateChange, prefix]);

  return useCallback(async <T,>(slot: string, request: () => Promise<T>, apply: (value: T) => void) => {
    const key = `${prefix}:${slot}`;
    const generation = (generations.current.get(key) ?? 0) + 1;
    generations.current.set(key, generation);
    activeKeys.current.add(key);
    onAsyncStateChange(key, { pending: true, invalid: false });
    try {
      const value = await request();
      if (generations.current.get(key) !== generation) return;
      apply(value);
      activeKeys.current.delete(key);
      onAsyncStateChange(key, undefined);
    } catch (reason) {
      if (generations.current.get(key) !== generation) return;
      onAsyncStateChange(key, { pending: false, invalid: true });
      toast(errorMessage(reason), { variant: "danger" });
    }
  }, [onAsyncStateChange, prefix]);
}

export function errorText(fieldErrors: Record<string, string[]>, prefix: string) {
  const messages = Object.entries(fieldErrors)
    .filter(([field]) => field === prefix || field.startsWith(`${prefix}.`))
    .flatMap(([, values]) => values);
  return messages.length > 0 ? [...new Set(messages)].join("；") : undefined;
}

export function requestConditionDraft(kind: ConditionKind): Promise<RuleCondition> {
  return callCommand(commands.ruleConditionDraft(kind));
}

export function requestActionDraft(kind: ActionKind): Promise<RuleAction> {
  return callCommand(commands.ruleActionDraft(kind));
}

export function requestMatchFieldDraft(kind: RuleMatchFieldKind): Promise<RuleMatchField> {
  return callCommand(commands.ruleMatchFieldDraft(kind));
}

export function requestMatchOperatorDraft(kind: RuleMatchOperatorKind): Promise<RuleMatchOperator> {
  return callCommand(commands.ruleMatchOperatorDraft(kind));
}

export function parseRuleByteInput(raw: string) {
  return callCommand(commands.ruleParseByteInput(raw));
}

export function parseRuleHeaderInput(raw: string) {
  return callCommand(commands.ruleParseHeaderInput(raw));
}

export function actionKind(action: RuleAction): ActionKind {
  return action.type === "terminal" ? action.action.type : action.type;
}

export const fieldLabels: Record<RuleMatchField["type"], string> = {
  terminal_ip: "终端 IP",
  certificate_fingerprint: "证书指纹",
  path_or_request_type: "路径 / 请求类型",
  json_path: "JSON Path",
};

export const actionLabels: Record<ActionKind, string> = {
  set_json_field: "设置 JSON 字段",
  replace_body_text: "替换 Body 文本",
  set_header: "设置 Header",
  delay: "延迟",
  jitter: "网络抖动",
  throttle: "带宽限速",
  intermittent: "间歇通断",
  pause: "暂停并进入断点",
  custom_http_status: "自定义 HTTP 状态码",
  reject_tls_handshake: "拒绝 TLS 握手",
  disconnect_before_upstream: "连接上游前断开",
  upstream_connect_timeout: "上游连接超时",
  upstream_write_timeout: "上游写入超时",
  upstream_read_timeout: "上游读取超时",
  drop_upstream_response: "丢弃上游响应",
  mock_response: "Mock 响应",
  invalid_json: "非法 JSON 响应",
  incorrect_content_length: "错误 Content-Length",
  truncate_response: "截断响应",
  disconnect_during_upstream_write: "上行 Body 中途断连",
  disconnect_during_downstream_write: "下行 Body 中途断连",
};

export const actionKinds = Object.keys(actionLabels) as ActionKind[];
