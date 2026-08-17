import { Alert, Button, Card, Chip, Label, ListBox, Select, Spinner, Switch } from "@heroui/react";
import { useState, type Key } from "react";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  ScriptedSocketProcessing,
  SocketDirection,
  SocketPayloadProcessing,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { bindPackage, exactPackageKey, matchingOption } from "./socket-listener-model";
import { SocketProtocolPackageDialog } from "./socket-protocol-package-dialog";

export interface ProtocolCatalogState {
  data?: ListenerProtocolPackageCatalogViewModel;
  error?: string;
  loading: boolean;
  refresh: () => Promise<void>;
}

export function SocketProcessingCard({ settings, catalog, locked, onChange }: {
  settings: SocketRelaySettings;
  catalog: ProtocolCatalogState;
  locked: boolean;
  onChange: (settings: SocketRelaySettings) => void;
}) {
  // 包切换可能按能力原子关闭开关，必须通过 live region 告知键盘/读屏用户。
  const [announcement, setAnnouncement] = useState("");
  const processing = settings.processing;
  if (!processing || processing.mode !== "scripted") return null;
  const scripted = processing.settings;
  const local = settings.topology.mode === "local_responder";
  // useIpcQuery 刷新时会保留旧 data。加载或错误状态下必须把旧快照视为不可用，
  // 否则用户可能在 Rust 正在重验/已经拒绝目录时继续修改方向开关。
  const selected = catalog.loading || catalog.error
    ? undefined
    : matchingOption(catalog.data, scripted.package);
  const selectedKey = exactPackageKey(scripted.package);
  const hasBoundPackage = scripted.package.id.length > 0 && scripted.package.version.length > 0;
  const missingFromCatalog = hasBoundPackage && !selected;
  const unavailableBound = missingFromCatalog
    && !catalog.loading
    && !catalog.error
    && Boolean(catalog.data);

  function selectPackage(key: Key | null) {
    const option = catalog.data?.options.find((item) => exactPackageKey(item.package) === key);
    if (!option) return;
    const nextProcessing = bindPackage(processing, option, local);
    const turnedOff = disabledCapabilities(scripted, nextProcessing, local);
    setAnnouncement(
      `已选择 ${option.name}`
      + (turnedOff.length > 0 ? `；因处理方案限制已关闭：${turnedOff.join("、")}` : "；现有处理选项保持不变"),
    );
    onChange({ ...settings, processing: nextProcessing });
  }

  function setDirection(direction: SocketDirection, field: "decode_enabled" | "encode_enabled", value: boolean) {
    const current = scripted[direction];
    onChange({ ...settings, processing: { mode: "scripted", settings: {
      ...scripted,
      [direction]: { ...current, [field]: value },
    } } });
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>4. 按协议处理</Card.Title>
        <Card.Description>选择处理方案，并按数据流向开启需要的读取或改写步骤。</Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4">
        {catalog.loading && <Spinner aria-label="正在读取 Listener 协议包目录" />}
        {catalog.error && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>协议包目录读取失败</Alert.Title><Alert.Description>{catalog.error}</Alert.Description>
          <Button size="sm" variant="outline" onPress={() => void catalog.refresh()}>重试</Button>
        </Alert.Content></Alert>}
        {!hasBoundPackage && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>尚未选择处理方案</Alert.Title>
          <Alert.Description>请选择一个可用的处理方案后再配置处理步骤。</Alert.Description>
        </Alert.Content></Alert>}
        {!catalog.loading && !catalog.error && catalog.data?.options.length === 0 && (
          <Alert status="warning"><Alert.Indicator /><Alert.Content>
            <Alert.Title>没有可绑定的协议包版本</Alert.Title>
            <Alert.Description>
              已安装 {catalog.data.installed_version_count} 个版本，其中 {catalog.data.unavailable_version_count} 个当前不可用。
              请先在协议包页面导入、修复或启用兼容版本。
            </Alert.Description>
          </Alert.Content></Alert>
        )}
        {unavailableBound && <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>当前处理方案已不可用</Alert.Title>
          <Alert.Description>原选择仍会保留，不会自动替换；请选择新的可用方案。</Alert.Description>
        </Alert.Content></Alert>}
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
          <Select aria-label="Socket 协议处理方案" selectedKey={selectedKey}
            isDisabled={locked || catalog.loading || Boolean(catalog.error) || catalog.data?.options.length === 0}
            onSelectionChange={selectPackage}>
            <Label>处理方案</Label>
            <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              {missingFromCatalog && <ListBox.Item id={selectedKey} isDisabled textValue="当前选择（不可用）">
                当前选择（不可用）
              </ListBox.Item>}
              {(catalog.data?.options ?? []).map((option) => (
                <ListBox.Item key={exactPackageKey(option.package)} id={exactPackageKey(option.package)} textValue={optionLabel(option)}>
                  <span>{optionLabel(option)}</span>
                </ListBox.Item>
              ))}
            </ListBox></Select.Popover>
          </Select>
        </div>
        {hasBoundPackage && <details className="rounded-xl border border-[var(--telemetry-line)] p-3">
          <summary className="cursor-pointer text-sm font-medium">高级技术信息</summary>
          <div className="mt-3 space-y-3">
            {selected && <PackageSummary option={selected} />}
            {!selected && <Chip variant="soft">{scripted.package.id}@{scripted.package.version}</Chip>}
            <SocketProtocolPackageDialog packageRef={scripted.package} disabled={locked || catalog.loading || Boolean(catalog.error)} />
          </div>
        </details>}
        {announcement && <p role="status" aria-live="polite" className="text-sm text-[var(--telemetry-muted)]">
          {announcement}
        </p>}
        {local ? (
          <div className="grid gap-4 xl:grid-cols-2">
            <DirectionSwitch label="读取应用请求内容" description="开启后可按请求字段匹配规则；关闭时仍可直接返回收到的原始数据。" selected={scripted.upstream.decode_enabled} disabled={locked || !selected || !selected.capabilities.upstream.decode} onChange={(value) => setDirection("upstream", "decode_enabled", value)} />
            <DirectionSwitch label="按规则生成应答内容" description="开启后根据处理结果生成返回给应用的数据。" selected={scripted.downstream.encode_enabled} disabled={locked || !selected || !selected.capabilities.downstream.encode} onChange={(value) => setDirection("downstream", "encode_enabled", value)} />
          </div>
        ) : (
          <div className="grid gap-4 xl:grid-cols-2">
            <DirectionCard title="应用发往远端" direction="upstream" settings={settings} option={selected} locked={locked} onChange={setDirection} />
            <DirectionCard title="远端返回应用" direction="downstream" settings={settings} option={selected} locked={locked} onChange={setDirection} />
          </div>
        )}
      </Card.Content>
    </Card>
  );
}

function DirectionCard({ title, direction, settings, option, locked, onChange }: {
  title: string; direction: SocketDirection; settings: SocketRelaySettings;
  option?: ListenerProtocolPackageOptionViewModel; locked: boolean;
  onChange: (direction: SocketDirection, field: "decode_enabled" | "encode_enabled", value: boolean) => void;
}) {
  const processing = settings.processing;
  if (!processing || processing.mode !== "scripted") return null;
  const value = processing.settings[direction];
  const capability = option?.capabilities[direction];
  return <section aria-label={title} className="space-y-3 rounded-xl border border-[var(--telemetry-line)] p-4">
    <h3 className="font-semibold">{title}</h3>
    <DirectionSwitch label={`读取${title}的内容`} description="开启后可按字段查看并匹配这一方向的数据。" selected={value.decode_enabled} disabled={locked || !capability?.decode} onChange={(next) => onChange(direction, "decode_enabled", next)} />
    <DirectionSwitch label={`按规则改写${title}的内容`} description="开启后把规则处理结果重新生成并发送。" selected={value.encode_enabled} disabled={locked || !capability?.encode} onChange={(next) => onChange(direction, "encode_enabled", next)} />
  </section>;
}

function DirectionSwitch({ label, description, selected, disabled, onChange }: {
  label: string; description: string; selected: boolean; disabled: boolean; onChange: (value: boolean) => void;
}) {
  return <div className="flex items-start justify-between gap-4 rounded-lg bg-[var(--telemetry-table-head)] p-3">
    <div><p className="text-sm font-medium">{label}</p><p className="mt-1 text-xs text-[var(--telemetry-muted)]">{description}</p></div>
    <Switch aria-label={label} isSelected={selected} isDisabled={disabled} onChange={onChange}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content></Switch>
  </div>;
}

function PackageSummary({ option }: { option: ListenerProtocolPackageOptionViewModel }) {
  return <div className="flex flex-wrap gap-2 text-sm"><Chip variant="soft">{option.package.id}@{option.package.version}</Chip>
    <Chip variant="soft">Schema {option.schema.id} v{option.schema.version}</Chip>
    <Chip variant="soft">{option.schema.fields.length} 个字段</Chip>
    <Chip variant="soft">Frame / Decode：双向</Chip>
    <Chip variant="soft">Encode：上行 {option.capabilities.upstream.encode ? "支持" : "不支持"}，下行 {option.capabilities.downstream.encode ? "支持" : "不支持"}</Chip>
    <Chip variant="soft" color={option.capabilities.display ? "success" : "default"}>Display：{option.capabilities.display ? "支持" : "未声明"}</Chip></div>;
}

function optionLabel(option: ListenerProtocolPackageOptionViewModel): string {
  return `${option.name} · ${option.package.version}`;
}

function disabledCapabilities(
  before: ScriptedSocketProcessing,
  after: SocketPayloadProcessing,
  local: boolean,
): string[] {
  if (after.mode !== "scripted") return [];
  const labels = local
    ? [["upstream", "decode_enabled", "读取应用请求内容"], ["downstream", "encode_enabled", "按规则生成应答内容"]] as const
    : [
      ["upstream", "decode_enabled", "读取应用发往远端的内容"],
      ["upstream", "encode_enabled", "改写应用发往远端的内容"],
      ["downstream", "decode_enabled", "读取远端返回应用的内容"],
      ["downstream", "encode_enabled", "改写远端返回应用的内容"],
    ] as const;
  return labels.flatMap(([direction, field, label]) =>
    before[direction][field] && !after.settings[direction][field] ? [label] : []);
}
