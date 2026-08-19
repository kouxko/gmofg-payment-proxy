import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FieldError, Input, Label, ListBox, Select, Spinner, TextArea, TextField } from "@heroui/react";
import type { DocumentValue, ProtocolRuleFieldCapability } from "@/generated/rust-types";
import { errorMessage } from "@/lib/ipc/client";
import { parseProtocolRuleValue, valueText } from "./protocol-rule-model";

export type ProtocolValueAsyncState = { pending: boolean; invalid: boolean };

const VALUE_PARSE_DELAY_MS = 180;

export function ProtocolRuleValueEditor({
  field,
  value,
  label,
  onChange,
  onAsyncStateChange,
  compact = false,
  disabled = false,
}: {
  field: ProtocolRuleFieldCapability;
  value: DocumentValue;
  label: string;
  onChange: (value: DocumentValue) => void;
  onAsyncStateChange: (state?: ProtocolValueAsyncState) => void;
  compact?: boolean;
  disabled?: boolean;
}) {
  const sourceKey = `${field.name}:${JSON.stringify(value)}`;
  const [input, setInput] = useState(() => ({ sourceKey, raw: valueText(value) }));
  const [parseError, setParseError] = useState<string>();
  const [pending, setPending] = useState(false);
  const generation = useRef(0);
  const parseTimer = useRef<number | undefined>(undefined);
  const pendingReported = useRef(false);
  const asyncStateRef = useRef(onAsyncStateChange);
  useEffect(() => {
    asyncStateRef.current = onAsyncStateChange;
  }, [onAsyncStateChange]);
  const sourceRaw = valueText(value);
  useLayoutEffect(() => {
    if (input.sourceKey === sourceKey) return;
    // 推迟同步以符合 React effect 约束。非法 raw 不会改变上游 sourceKey，因此
    // 普通父级重渲染不会覆盖；规则重载或切换字段时才采用新的权威值。
    generation.current += 1;
    if (parseTimer.current !== undefined) window.clearTimeout(parseTimer.current);
    pendingReported.current = false;
    asyncStateRef.current(undefined);
    const task = window.setTimeout(() => {
      setInput({ sourceKey, raw: sourceRaw });
      setParseError(undefined);
      setPending(false);
    }, 0);
    return () => window.clearTimeout(task);
  }, [input.sourceKey, sourceKey, sourceRaw]);
  useEffect(() => () => {
    generation.current += 1;
    if (parseTimer.current !== undefined) window.clearTimeout(parseTimer.current);
    pendingReported.current = false;
    asyncStateRef.current(undefined);
  }, []);
  const raw = input.sourceKey === sourceKey ? input.raw : sourceRaw;
  if (field.type === "bool") {
    return (
      <div className="grid gap-1">
        <Label className={compact ? "sr-only" : undefined}>{label}</Label>
        <Select
          aria-label={label}
          isDisabled={disabled}
          isInvalid={Boolean(parseError)}
          selectedKey={raw === "true" ? "true" : "false"}
          onSelectionChange={(key) => commitRaw(String(key))}
        >
          <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>
            <ListBox.Item id="true" textValue="true">true</ListBox.Item>
            <ListBox.Item id="false" textValue="false">false</ListBox.Item>
          </ListBox></Select.Popover>
        </Select>
        {pending && <Spinner aria-label={`正在解析${label}`} className={compact ? "sr-only" : undefined} size="sm" />}
        {parseError && <FieldError>{parseError}</FieldError>}
      </div>
    );
  }
  return (
    <TextField className="min-w-0" isDisabled={disabled} isInvalid={Boolean(parseError)}>
      <Label className={compact ? "sr-only" : undefined}>{label}</Label>
      {field.type === "blob" ? <TextArea
        aria-description="使用两位十六进制表示每个字节，最多 64 KiB"
        className="min-h-24 font-mono"
        placeholder="例如：9F 26 08 A1 B2 C3 D4 E5 F6 07 08"
        value={raw}
        onChange={(event) => updateRaw(event.target.value)}
      /> : <Input
        aria-description={field.type === "int" ? "请输入 JavaScript 安全整数范围内的十进制整数" : undefined}
        inputMode={field.type === "int" ? "numeric" : "text"}
        placeholder={compact ? label : undefined}
        value={raw}
        onChange={(event) => updateRaw(event.target.value)}
      />}
      {field.type === "blob" && <p className="text-xs text-[var(--telemetry-muted)]">使用两位 Hex 表示一个字节，可用空格、冒号或连字符分隔；当前 {value.type === "blob" ? value.value.length : 0} 字节。</p>}
      {pending && <Spinner aria-label={`正在解析${label}`} className={compact ? "sr-only" : undefined} size="sm" />}
      {parseError && <FieldError>{parseError}</FieldError>}
    </TextField>
  );

  function updateRaw(nextRaw: string) {
    const requestGeneration = stageRaw(nextRaw);
    parseTimer.current = window.setTimeout(() => {
      parseTimer.current = undefined;
      void parseRaw(nextRaw, requestGeneration);
    }, VALUE_PARSE_DELAY_MS);
  }

  function commitRaw(nextRaw: string) {
    const requestGeneration = stageRaw(nextRaw);
    void parseRaw(nextRaw, requestGeneration);
  }

  function stageRaw(nextRaw: string) {
    const requestGeneration = ++generation.current;
    if (parseTimer.current !== undefined) window.clearTimeout(parseTimer.current);
    parseTimer.current = undefined;
    setInput({ sourceKey, raw: nextRaw });
    setPending(true);
    setParseError(undefined);
    if (!pendingReported.current) {
      pendingReported.current = true;
      onAsyncStateChange({ pending: true, invalid: false });
    }
    return requestGeneration;
  }

  async function parseRaw(nextRaw: string, requestGeneration: number) {
    try {
      const parsed = await parseProtocolRuleValue(field.type, nextRaw);
      if (requestGeneration !== generation.current) return;
      onChange(parsed);
      setPending(false);
      pendingReported.current = false;
      onAsyncStateChange(undefined);
    } catch (reason) {
      if (requestGeneration !== generation.current) return;
      setParseError(errorMessage(reason));
      setPending(false);
      pendingReported.current = false;
      onAsyncStateChange({ pending: false, invalid: true });
    }
  }
}
