import { Input, Label, ListBox, Select, TextArea, TextField } from "@heroui/react";
import type { RuleActionKind } from "@/generated/rust-types";
import type { HttpActionDraft } from "./rule-http-action-parameters";

export function HttpActionParametersForm(props: {
  draft: HttpActionDraft;
  onChange: (draft: HttpActionDraft) => void;
}) {
  const update = (change: Partial<HttpActionDraft>) => props.onChange({ ...props.draft, ...change });
  const kind = props.draft.kind;
  if (kind === "replace_body_text") {
    return <TextParameter label="Body 文本" description="替换当前处理阶段的完整 HTTP Body。" value={props.draft.bodyText} onChange={(bodyText) => update({ bodyText })} />;
  }
  if (kind === "delay") {
    return <IntegerParameter label="延迟时间（毫秒）" description="命中规则后等待的精确时长。" value={props.draft.milliseconds} onChange={(milliseconds) => update({ milliseconds })} />;
  }
  if (kind === "jitter") {
    return <div className="grid items-end gap-3 sm:grid-cols-2">
      <IntegerParameter label="最小抖动（毫秒）" description="每次抖动的最短等待时间。" value={props.draft.minimumMilliseconds} onChange={(minimumMilliseconds) => update({ minimumMilliseconds })} />
      <IntegerParameter label="最大抖动（毫秒）" description="每次抖动的最长等待时间。" value={props.draft.maximumMilliseconds} onChange={(maximumMilliseconds) => update({ maximumMilliseconds })} />
      <Select aria-label="抖动方式" selectedKey={props.draft.jitterScope || null} onSelectionChange={(key) => update({ jitterScope: String(key) as HttpActionDraft["jitterScope"] })}>
        <Label>抖动方式</Label>
        <ClippedSelectTrigger />
        <Select.Popover><ListBox>
          <ListBox.Item id="before_message" textValue="整条消息前">整条消息前</ListBox.Item>
          <ListBox.Item id="per_chunk" textValue="每个分块">每个分块</ListBox.Item>
        </ListBox></Select.Popover>
      </Select>
    </div>;
  }
  if (kind === "throttle") {
    return <div className="grid items-end gap-3 sm:grid-cols-2">
      <IntegerParameter label="速率（B/s）" description="每秒最多发送的 Body 字节数。" value={props.draft.bytesPerSecond} onChange={(bytesPerSecond) => update({ bytesPerSecond })} />
      <IntegerParameter label="分块大小（字节）" description="每个发送分块的最大字节数。" value={props.draft.chunkBytes} onChange={(chunkBytes) => update({ chunkBytes })} />
    </div>;
  }
  if (kind === "intermittent") {
    return <div className="grid items-end gap-3 sm:grid-cols-2">
      <IntegerParameter label="可用窗口（毫秒）" description="允许发送数据的连续时长。" value={props.draft.availableMilliseconds} onChange={(availableMilliseconds) => update({ availableMilliseconds })} />
      <IntegerParameter label="阻断窗口（毫秒）" description="暂停发送数据的连续时长。" value={props.draft.blockedMilliseconds} onChange={(blockedMilliseconds) => update({ blockedMilliseconds })} />
    </div>;
  }
  if (kind === "custom_http_status") {
    return <IntegerParameter label="HTTP 状态码" description="返回给客户端的 HTTP 状态码。" value={props.draft.status} onChange={(status) => update({ status })} />;
  }
  if (isTimeoutKind(kind)) {
    return <IntegerParameter label="超时时间（毫秒）" description="保持对应网络阶段直至此时长结束。" value={props.draft.milliseconds} onChange={(milliseconds) => update({ milliseconds })} />;
  }
  if (kind === "drop_upstream_response") {
    return <Select aria-label="丢弃方式" selectedKey={props.draft.dropResponseMode || null} onSelectionChange={(key) => update({ dropResponseMode: String(key) as HttpActionDraft["dropResponseMode"] })}>
      <Label>丢弃方式</Label>
      <ClippedSelectTrigger />
      <Select.Popover><ListBox>
        <ListBox.Item id="read_complete_response" textValue="读取完整响应后丢弃">读取完整响应后丢弃</ListBox.Item>
        <ListBox.Item id="close_after_request_write" textValue="写完请求后立即断开">写完请求后立即断开</ListBox.Item>
      </ListBox></Select.Popover>
    </Select>;
  }
  if (kind === "invalid_json") {
    return <TextParameter
      label="非法 JSON Body 字节"
      description="按十进制输入 0–255 的字节，多个字节使用逗号分隔。"
      value={props.draft.invalidJsonBytes}
      onChange={(invalidJsonBytes) => update({ invalidJsonBytes })}
    />;
  }
  if (kind === "incorrect_content_length") {
    return <IntegerParameter signed label="长度偏移量（字节）" description="声明的 Content-Length 相对真实 Body 长度的有符号偏移。" value={props.draft.delta} onChange={(delta) => update({ delta })} />;
  }
  if (kind === "truncate_response") {
    return <IntegerParameter label="发送字节数（字节）" description="仅发送响应 Body 的前 N 字节后断开。" value={props.draft.bytes} onChange={(bytes) => update({ bytes })} />;
  }
  if (isDisconnectDuringWriteKind(kind)) {
    return <IntegerParameter label="断连偏移（字节）" description="成功发送前 N 字节后立即中止连接。" value={props.draft.afterBytes} onChange={(afterBytes) => update({ afterBytes })} />;
  }
  return null;
}

function IntegerParameter(props: {
  label: string;
  description: string;
  value: string;
  signed?: boolean;
  onChange: (value: string) => void;
}) {
  return <div className="space-y-1">
    <TextField>
      <Label>{props.label}</Label>
      <Input
        aria-label={props.label}
        className="h-10 w-full py-0"
        inputMode="numeric"
        min={props.signed ? undefined : 0}
        step={1}
        type="number"
        value={props.value}
        onChange={(event) => props.onChange(event.target.value)}
      />
    </TextField>
    <p className="text-xs text-[var(--telemetry-muted)]">{props.description}</p>
  </div>;
}

function TextParameter(props: {
  label: string;
  description: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return <TextField className="w-full">
    <Label>{props.label}</Label>
    <TextArea aria-label={props.label} className="min-h-24 w-full" value={props.value} onChange={(event) => props.onChange(event.target.value)} />
    <p className="text-xs text-[var(--telemetry-muted)]">{props.description}</p>
  </TextField>;
}

function ClippedSelectTrigger() {
  return <Select.Trigger className="h-10 min-h-10 w-full min-w-0 overflow-hidden">
    <Select.Value className="min-w-0 flex-1 truncate whitespace-nowrap" />
    <Select.Indicator className="shrink-0" />
  </Select.Trigger>;
}

function isTimeoutKind(kind: RuleActionKind | "") {
  return kind === "upstream_connect_timeout"
    || kind === "upstream_write_timeout"
    || kind === "upstream_read_timeout";
}

function isDisconnectDuringWriteKind(kind: RuleActionKind | "") {
  return kind === "disconnect_during_upstream_write" || kind === "disconnect_during_downstream_write";
}
