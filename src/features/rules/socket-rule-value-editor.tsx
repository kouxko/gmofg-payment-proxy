import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FieldError, Input, Label, ListBox, Select, Spinner, TextArea, TextField } from "@heroui/react";
import type { DocumentValue, SocketRuleFieldCapability } from "@/generated/rust-types";
import { errorMessage } from "@/lib/ipc/client";
import { parseSocketRuleValue, valueText } from "./socket-rule-model";

export type SocketValueAsyncState = { pending: boolean; invalid: boolean };

export function SocketRuleValueEditor({
  field,
  value,
  label,
  onChange,
  onAsyncStateChange,
  disabled = false,
}: {
  field: SocketRuleFieldCapability;
  value: DocumentValue;
  label: string;
  onChange: (value: DocumentValue) => void;
  onAsyncStateChange: (state?: SocketValueAsyncState) => void;
  disabled?: boolean;
}) {
  const sourceKey = `${field.name}:${JSON.stringify(value)}`;
  const [input, setInput] = useState(() => ({ sourceKey, raw: valueText(value) }));
  const [parseError, setParseError] = useState<string>();
  const [pending, setPending] = useState(false);
  const generation = useRef(0);
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
    asyncStateRef.current(undefined);
  }, []);
  const raw = input.sourceKey === sourceKey ? input.raw : sourceRaw;
  if (field.type === "bool") {
    return (
      <div className="grid gap-1">
        <Label>{label}</Label>
        <Select
          aria-label={label}
          isDisabled={disabled}
          isInvalid={Boolean(parseError)}
          selectedKey={raw === "true" ? "true" : "false"}
          onSelectionChange={(key) => void commitRaw(String(key))}
        >
          <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>
            <ListBox.Item id="true" textValue="true">true</ListBox.Item>
            <ListBox.Item id="false" textValue="false">false</ListBox.Item>
          </ListBox></Select.Popover>
        </Select>
        {pending && <Spinner aria-label={`正在解析${label}`} size="sm" />}
        {parseError && <FieldError>{parseError}</FieldError>}
      </div>
    );
  }
  return (
    <TextField isDisabled={disabled} isInvalid={Boolean(parseError)}>
      <Label>{label}</Label>
      {field.type === "blob" ? <TextArea
        aria-description="使用两位十六进制表示每个字节，最多 64 KiB"
        className="min-h-24 font-mono"
        placeholder="例如：9F 26 08 A1 B2 C3 D4 E5 F6 07 08"
        value={raw}
        onChange={(event) => updateRaw(event.target.value)}
      /> : <Input
        aria-description={field.type === "int" ? "请输入 JavaScript 安全整数范围内的十进制整数" : undefined}
        inputMode={field.type === "int" ? "numeric" : "text"}
        value={raw}
        onChange={(event) => updateRaw(event.target.value)}
      />}
      {field.type === "int" && <p className="text-xs text-[var(--telemetry-muted)]">十进制整数，范围 −9,007,199,254,740,991 至 9,007,199,254,740,991。</p>}
      {field.type === "blob" && <p className="text-xs text-[var(--telemetry-muted)]">使用两位 Hex 表示一个字节，可用空格、冒号或连字符分隔；当前 {value.type === "blob" ? value.value.length : 0} 字节。</p>}
      {pending && <Spinner aria-label={`正在解析${label}`} size="sm" />}
      {parseError && <FieldError>{parseError}</FieldError>}
    </TextField>
  );

  function updateRaw(nextRaw: string) {
    void commitRaw(nextRaw);
  }

  async function commitRaw(nextRaw: string) {
    const requestGeneration = ++generation.current;
    setInput({ sourceKey, raw: nextRaw });
    setPending(true);
    setParseError(undefined);
    onAsyncStateChange({ pending: true, invalid: false });
    try {
      const parsed = await parseSocketRuleValue(field.type, nextRaw);
      if (requestGeneration !== generation.current) return;
      onChange(parsed);
      setPending(false);
      onAsyncStateChange(undefined);
    } catch (reason) {
      if (requestGeneration !== generation.current) return;
      setParseError(errorMessage(reason));
      setPending(false);
      onAsyncStateChange({ pending: false, invalid: true });
    }
  }
}
