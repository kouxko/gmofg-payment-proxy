"use client";

/**
 * Workspace 中可复用策略的纯展示编辑器。
 *
 * 这里仅把用户输入投影回 Rust 生成的 DTO，不生成领域 ID、不读取文件、不解析证书，
 * 也不执行 JSONPath、编解码或断言。新增组件必须调用 Rust 的
 * `workspace_component_new`，最终合法性统一由 `workspace_validate` 决定。
 */

import {
  Button,
  Card,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Switch,
  Tabs,
} from "@heroui/react";
import { Plus, TrashBin } from "@gravity-ui/icons";
import type {
  BodyCodecKind,
  BodyDirection,
  CertificateReferenceKind,
  ConnectionFaultAction,
  ProxyWorkspace,
  ResponseAssertionKind,
} from "@/generated/rust-types";

type ComponentKind =
  | "body_codec"
  | "metadata_extractor"
  | "response_assertion"
  | "fault_preset"
  | "certificate_reference";

const codecLabels: Record<BodyCodecKind, string> = {
  raw: "Raw（原始字节）",
  utf8: "UTF-8",
  shift_jis: "Shift-JIS",
};

const certificateKindLabels: Record<CertificateReferenceKind, string> = {
  mitm_root_ca: "MITM Root CA",
  reverse_server_identity: "Reverse 服务端身份",
  downstream_client_trust: "下游客户端信任",
  upstream_client_identity: "上游客户端身份",
  upstream_server_trust: "上游服务端信任",
};

export function WorkspaceComponentsEditor({
  workspace,
  onChange,
  onAdd,
  onIntent,
  disabled,
}: {
  workspace: ProxyWorkspace;
  onChange: (workspace: ProxyWorkspace) => void;
  onAdd: (kind: ComponentKind) => void;
  onIntent: (kind: ComponentKind, id: string, operation: string, value: string) => void;
  disabled: boolean;
}) {
  return (
    <Tabs aria-label="Workspace 策略配置" defaultSelectedKey="codecs">
      <Tabs.ListContainer><Tabs.List>
        <Tabs.Tab id="codecs">Body Codec</Tabs.Tab>
        <Tabs.Tab id="extractors">元数据提取</Tabs.Tab>
        <Tabs.Tab id="assertions">响应断言</Tabs.Tab>
        <Tabs.Tab id="certificates">证书引用</Tabs.Tab>
        <Tabs.Tab id="faults">连接故障预设</Tabs.Tab>
      </Tabs.List></Tabs.ListContainer>

      <Tabs.Panel id="codecs" className="space-y-3 pt-4">
        <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("body_codec")}><Plus className="size-4" />新增 Body Codec</Button>
        {workspace.body_codec_policies.map((policy, index) => (
          <Card key={policy.id}><Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
            <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1"><strong>Body Codec {index + 1}</strong><code className="text-xs text-[var(--telemetry-muted)]">{policy.id}</code><Button className="ml-auto" isIconOnly aria-label={`删除 Body Codec ${index + 1}`} variant="danger-soft" onPress={() => onIntent("body_codec", policy.id, "delete", "")}><TrashBin className="size-4" /></Button></div>
            <div className="grid gap-1"><Label>名称</Label><Input value={policy.name} onChange={(event) => onChange({ ...workspace, body_codec_policies: workspace.body_codec_policies.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item) })} /></div>
            <div className="grid gap-1"><Label>代理入口 ID（逗号分隔；留空表示未绑定）</Label><Input key={`${policy.id}:${policy.listener_ids.join(",")}`} defaultValue={policy.listener_ids.join(", ")} onBlur={(event) => onIntent("body_codec", policy.id, "listener_ids", event.target.value)} /></div>
            <Select aria-label={`Body Codec ${index + 1} 编码`} selectedKey={policy.codec} onSelectionChange={(key) => onChange({ ...workspace, body_codec_policies: workspace.body_codec_policies.map((item, itemIndex) => itemIndex === index ? { ...item, codec: key as BodyCodecKind } : item) })}><Label>编码</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{Object.entries(codecLabels).map(([id, label]) => <ListBox.Item key={id} id={id}>{label}</ListBox.Item>)}</ListBox></Select.Popover></Select>
            <Select aria-label={`Body Codec ${index + 1} 方向`} selectedKey={policy.direction} onSelectionChange={(key) => onChange({ ...workspace, body_codec_policies: workspace.body_codec_policies.map((item, itemIndex) => itemIndex === index ? { ...item, direction: key as BodyDirection } : item) })}><Label>方向</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="request">请求</ListBox.Item><ListBox.Item id="response">响应</ListBox.Item><ListBox.Item id="both">请求与响应</ListBox.Item></ListBox></Select.Popover></Select>
          </Card.Content></Card>
        ))}
      </Tabs.Panel>

      <Tabs.Panel id="extractors" className="space-y-3 pt-4">
        <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("metadata_extractor")}><Plus className="size-4" />新增提取器</Button>
        {workspace.metadata_extractors.map((extractor, index) => (
          <Card key={extractor.id}><Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
            <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1"><strong>提取器 {index + 1}</strong><code className="text-xs text-[var(--telemetry-muted)]">{extractor.id}</code><Button className="ml-auto" isIconOnly aria-label={`删除提取器 ${index + 1}`} variant="danger-soft" onPress={() => onIntent("metadata_extractor", extractor.id, "delete", "")}><TrashBin className="size-4" /></Button></div>
            <div className="grid gap-1"><Label>名称（作为元数据 Key）</Label><Input value={extractor.name} onChange={(event) => onChange({ ...workspace, metadata_extractors: workspace.metadata_extractors.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item) })} /></div>
            <div className="grid gap-1"><Label>代理入口 ID（逗号分隔）</Label><Input key={`${extractor.id}:${extractor.listener_ids.join(",")}`} defaultValue={extractor.listener_ids.join(", ")} onBlur={(event) => onIntent("metadata_extractor", extractor.id, "listener_ids", event.target.value)} /></div>
            <Select aria-label={`提取器 ${index + 1} 来源`} selectedKey={extractor.source.kind} onSelectionChange={(key) => onIntent("metadata_extractor", extractor.id, "variant", String(key))}><Label>来源</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="header">Header</ListBox.Item><ListBox.Item id="json_path">JSONPath</ListBox.Item><ListBox.Item id="body_text">Body 文本</ListBox.Item><ListBox.Item id="fixed_value">固定值</ListBox.Item></ListBox></Select.Popover></Select>
            <div className="grid gap-1"><Label>参数</Label><Input disabled={extractor.source.kind === "body_text"} value={extractor.source.kind === "header" ? extractor.source.name : extractor.source.kind === "json_path" ? extractor.source.path : extractor.source.kind === "fixed_value" ? extractor.source.value : ""} onChange={(event) => { const value = event.target.value; const source = extractor.source.kind === "header" ? { ...extractor.source, name: value } : extractor.source.kind === "json_path" ? { ...extractor.source, path: value } : extractor.source.kind === "fixed_value" ? { ...extractor.source, value } : extractor.source; onChange({ ...workspace, metadata_extractors: workspace.metadata_extractors.map((item, itemIndex) => itemIndex === index ? { ...item, source } : item) }); }} placeholder="Header 名 / $.path / 固定值" /></div>
          </Card.Content></Card>
        ))}
      </Tabs.Panel>

      <Tabs.Panel id="assertions" className="space-y-3 pt-4">
        <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("response_assertion")}><Plus className="size-4" />新增响应断言</Button>
        {workspace.response_assertions.map((assertion, index) => (
          <Card key={assertion.id}><Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
            <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1"><strong>响应断言 {index + 1}</strong><code className="text-xs text-[var(--telemetry-muted)]">{assertion.id}</code><Switch className="ml-auto" isSelected={assertion.enabled} onChange={(enabled) => onChange({ ...workspace, response_assertions: workspace.response_assertions.map((item, itemIndex) => itemIndex === index ? { ...item, enabled } : item) })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用</span></Switch.Content></Switch><Button isIconOnly aria-label={`删除响应断言 ${index + 1}`} variant="danger-soft" onPress={() => onIntent("response_assertion", assertion.id, "delete", "")}><TrashBin className="size-4" /></Button></div>
            <div className="grid gap-1"><Label>名称</Label><Input value={assertion.name} onChange={(event) => onChange({ ...workspace, response_assertions: workspace.response_assertions.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item) })} /></div>
            <div className="grid gap-1"><Label>代理入口 ID（逗号分隔）</Label><Input key={`${assertion.id}:${assertion.listener_ids.join(",")}`} defaultValue={assertion.listener_ids.join(", ")} onBlur={(event) => onIntent("response_assertion", assertion.id, "listener_ids", event.target.value)} /></div>
            <Select aria-label={`响应断言 ${index + 1} 类型`} selectedKey={assertion.assertion.kind} onSelectionChange={(key) => onIntent("response_assertion", assertion.id, "variant", String(key))}><Label>断言类型</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="http_status_equals">HTTP 状态码等于</ListBox.Item><ListBox.Item id="header_equals">Header 等于</ListBox.Item><ListBox.Item id="json_path_equals">JSONPath 等于</ListBox.Item><ListBox.Item id="body_text_contains">Body 文本包含</ListBox.Item><ListBox.Item id="body_length_equals">Body 长度等于</ListBox.Item><ListBox.Item id="body_sha256_equals">Body SHA-256 等于</ListBox.Item></ListBox></Select.Popover></Select>
            <AssertionInputs assertion={assertion.assertion} onChange={(value) => onChange({ ...workspace, response_assertions: workspace.response_assertions.map((item, itemIndex) => itemIndex === index ? { ...item, assertion: value } : item) })} />
          </Card.Content></Card>
        ))}
      </Tabs.Panel>

      <Tabs.Panel id="certificates" className="space-y-3 pt-4">
        <p className="text-sm text-[var(--telemetry-muted)]">Workspace 只保存安全引用。PEM 可使用 file:/path；PKCS#12 身份使用 pkcs12:/path?password_env=变量名，密码不会进入文档。</p>
        <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("certificate_reference")}><Plus className="size-4" />新增证书引用</Button>
        {workspace.certificate_references.map((reference, index) => (
          <Card key={reference.id}><Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
            <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1"><strong>证书引用 {index + 1}</strong><code className="text-xs text-[var(--telemetry-muted)]">{reference.id}</code><Button className="ml-auto" isIconOnly aria-label={`删除证书引用 ${index + 1}`} variant="danger-soft" onPress={() => onIntent("certificate_reference", reference.id, "delete", "")}><TrashBin className="size-4" /></Button></div>
            <div className="grid gap-1"><Label>名称</Label><Input value={reference.label} onChange={(event) => onChange({ ...workspace, certificate_references: workspace.certificate_references.map((item, itemIndex) => itemIndex === index ? { ...item, label: event.target.value } : item) })} /></div>
            <Select aria-label={`证书引用 ${index + 1} 类型`} selectedKey={reference.kind} onSelectionChange={(key) => onChange({ ...workspace, certificate_references: workspace.certificate_references.map((item, itemIndex) => itemIndex === index ? { ...item, kind: key as CertificateReferenceKind } : item) })}><Label>用途</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{Object.entries(certificateKindLabels).map(([id, label]) => <ListBox.Item key={id} id={id}>{label}</ListBox.Item>)}</ListBox></Select.Popover></Select>
            <div className="col-span-2 grid gap-1 max-[700px]:col-span-1"><Label>安全引用</Label><Input value={reference.reference} onChange={(event) => onChange({ ...workspace, certificate_references: workspace.certificate_references.map((item, itemIndex) => itemIndex === index ? { ...item, reference: event.target.value } : item) })} placeholder="file:/path/to/chain-and-key.pem" /></div>
          </Card.Content></Card>
        ))}
      </Tabs.Panel>

      <Tabs.Panel id="faults" className="space-y-3 pt-4">
        <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("fault_preset")}><Plus className="size-4" />新增连接故障预设</Button>
        {workspace.fault_presets.map((preset, index) => {
          const action = preset.connection_actions[0];
          return <Card key={preset.id}><Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
            <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1"><strong>故障预设 {index + 1}</strong><code className="text-xs text-[var(--telemetry-muted)]">{preset.id}</code><Button className="ml-auto" isIconOnly aria-label={`删除故障预设 ${index + 1}`} variant="danger-soft" onPress={() => onIntent("fault_preset", preset.id, "delete", "")}><TrashBin className="size-4" /></Button></div>
            <div className="grid gap-1"><Label>名称</Label><Input value={preset.name} onChange={(event) => onChange({ ...workspace, fault_presets: workspace.fault_presets.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item) })} /></div>
            <div className="grid gap-1"><Label>说明</Label><Input value={preset.description} onChange={(event) => onChange({ ...workspace, fault_presets: workspace.fault_presets.map((item, itemIndex) => itemIndex === index ? { ...item, description: event.target.value } : item) })} /></div>
            <Select aria-label={`故障预设 ${index + 1} 动作`} selectedKey={action?.kind} onSelectionChange={(key) => onIntent("fault_preset", preset.id, "variant", String(key))}><Label>连接动作</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="delay">连接延迟</ListBox.Item><ListBox.Item id="reject">拒绝连接</ListBox.Item><ListBox.Item id="rate_limit">连接限速</ListBox.Item><ListBox.Item id="close_after_bytes">指定字节后关闭</ListBox.Item><ListBox.Item id="half_close_after_bytes">指定字节后 half-close</ListBox.Item><ListBox.Item id="idle_timeout">空闲超时</ListBox.Item></ListBox></Select.Popover></Select>
            {action ? <FaultValue action={action} onChange={(value) => onChange({ ...workspace, fault_presets: workspace.fault_presets.map((item, itemIndex) => itemIndex === index ? { ...item, connection_actions: [value] } : item) })} /> : <p className="text-sm text-danger">缺少连接动作，请重新选择。</p>}
          </Card.Content></Card>;
        })}
      </Tabs.Panel>
    </Tabs>
  );
}

function AssertionInputs({ assertion, onChange }: { assertion: ResponseAssertionKind; onChange: (value: ResponseAssertionKind) => void }) {
  if (assertion.kind === "http_status_equals") return <NumberField minValue={100} maxValue={599} value={assertion.expected} onChange={(expected) => onChange({ ...assertion, expected })}><Label>期望状态码</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>;
  if (assertion.kind === "body_length_equals") return <NumberField minValue={0} value={assertion.expected} onChange={(expected) => onChange({ ...assertion, expected })}><Label>期望字节数</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>;
  if (assertion.kind === "header_equals") return <div className="grid grid-cols-2 gap-2"><div className="grid gap-1"><Label>Header 名</Label><Input value={assertion.name} onChange={(event) => onChange({ ...assertion, name: event.target.value })} /></div><div className="grid gap-1"><Label>期望值</Label><Input value={assertion.expected} onChange={(event) => onChange({ ...assertion, expected: event.target.value })} /></div></div>;
  if (assertion.kind === "json_path_equals") return <div className="grid grid-cols-2 gap-2"><div className="grid gap-1"><Label>JSONPath</Label><Input value={assertion.path} onChange={(event) => onChange({ ...assertion, path: event.target.value })} /></div><div className="grid gap-1"><Label>期望值</Label><Input value={String(assertion.expected ?? "")} onChange={(event) => onChange({ ...assertion, expected: event.target.value })} /></div></div>;
  if (assertion.kind === "body_text_contains") return <div className="grid gap-1"><Label>Body 必须包含</Label><Input value={assertion.expected} onChange={(event) => onChange({ ...assertion, expected: event.target.value })} /></div>;
  return <div className="grid gap-1"><Label>SHA-256（64 位十六进制）</Label><Input value={assertion.expected_hex} onChange={(event) => onChange({ ...assertion, expected_hex: event.target.value })} /></div>;
}

function FaultValue({ action, onChange }: { action: ConnectionFaultAction; onChange: (value: ConnectionFaultAction) => void }) {
  if (action.kind === "reject") return <p className="self-end text-sm text-[var(--telemetry-muted)]">该动作没有参数。</p>;
  const value = action.kind === "delay" || action.kind === "idle_timeout" ? action.milliseconds : action.kind === "rate_limit" ? action.bytes_per_second : action.bytes;
  const label = action.kind === "delay" || action.kind === "idle_timeout" ? "毫秒" : action.kind === "rate_limit" ? "字节/秒" : "字节数";
  return <NumberField minValue={1} value={value} onChange={(next) => onChange(action.kind === "delay" || action.kind === "idle_timeout" ? { ...action, milliseconds: next } : action.kind === "rate_limit" ? { ...action, bytes_per_second: next } : { ...action, bytes: next })}><Label>{label}</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>;
}
