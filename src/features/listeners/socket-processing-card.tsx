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
      `已绑定 ${option.package.id}@${option.package.version}`
      + (turnedOff.length > 0 ? `；因新版本能力限制已关闭：${turnedOff.join("、")}` : "；方向开关保持不变"),
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
        <Card.Title>4. Scripted 协议处理</Card.Title>
        <Card.Description>精确绑定一个可用协议包版本；Display 跟随同方向 Encode，不提供独立开关。</Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4">
        {catalog.loading && <Spinner aria-label="正在读取 Listener 协议包目录" />}
        {catalog.error && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>协议包目录读取失败</Alert.Title><Alert.Description>{catalog.error}</Alert.Description>
          <Button size="sm" variant="outline" onPress={() => void catalog.refresh()}>重试</Button>
        </Alert.Content></Alert>}
        {!hasBoundPackage && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>尚未绑定协议包</Alert.Title>
          <Alert.Description>Scripted 模式必须选择一个健康且兼容的精确协议包版本后才能启用处理开关。</Alert.Description>
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
          <Alert.Title>当前绑定版本已不可用</Alert.Title>
          <Alert.Description>仍保留 {scripted.package.id}@{scripted.package.version}，不会静默换包；请选择新的可用精确版本。</Alert.Description>
        </Alert.Content></Alert>}
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
          <Select aria-label="Socket 精确协议包版本" selectedKey={selectedKey}
            isDisabled={locked || catalog.loading || Boolean(catalog.error) || catalog.data?.options.length === 0}
            onSelectionChange={selectPackage}>
            <Label>精确协议包版本</Label>
            <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              {missingFromCatalog && <ListBox.Item id={selectedKey} isDisabled textValue={`${scripted.package.id}@${scripted.package.version}（不可用）`}>
                {scripted.package.id}@{scripted.package.version}（不可用）
              </ListBox.Item>}
              {(catalog.data?.options ?? []).map((option) => (
                <ListBox.Item key={exactPackageKey(option.package)} id={exactPackageKey(option.package)} textValue={optionLabel(option)}>
                  <div className="grid gap-0.5"><span>{option.name}</span>
                    <span className="font-mono text-xs text-[var(--telemetry-muted)]">{option.package.id}@{option.package.version} · {option.schema.id} v{option.schema.version}</span>
                  </div>
                </ListBox.Item>
              ))}
            </ListBox></Select.Popover>
          </Select>
          {hasBoundPackage && <SocketProtocolPackageDialog packageRef={scripted.package} />}
        </div>
        {selected && <PackageSummary option={selected} />}
        {announcement && <p role="status" aria-live="polite" className="text-sm text-[var(--telemetry-muted)]">
          {announcement}
        </p>}
        {local ? (
          <div className="grid gap-4 lg:grid-cols-2">
            <DirectionSwitch label="Request Decode" description="关闭时 Request 仅展示 Hex；Always + SetField 仍可在空 Response Document 构造返回值。" selected={scripted.upstream.decode_enabled} disabled={locked || !selected || !selected.capabilities.upstream.decode} onChange={(value) => setDirection("upstream", "decode_enabled", value)} />
            <DirectionSwitch label="Response Encode" description="关闭时向 App 返回 Raw Echo；开启时 Encode Response Document，声明 Display 时同时生成协议视图。" selected={scripted.downstream.encode_enabled} disabled={locked || !selected || !selected.capabilities.downstream.encode} onChange={(value) => setDirection("downstream", "encode_enabled", value)} />
          </div>
        ) : (
          <div className="grid gap-4 xl:grid-cols-2">
            <DirectionCard title="App → Server" direction="upstream" settings={settings} option={selected} locked={locked} onChange={setDirection} />
            <DirectionCard title="Server → App" direction="downstream" settings={settings} option={selected} locked={locked} onChange={setDirection} />
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
    <DirectionSwitch label={`${title} Decode`} description="完整 Frame 解码为 Document，关闭时该方向不执行 Document 规则。" selected={value.decode_enabled} disabled={locked || !capability?.decode} onChange={(next) => onChange(direction, "decode_enabled", next)} />
    <DirectionSwitch label={`${title} Encode`} description="编码 Document 后写出；Display 若声明则随 Encode 执行。" selected={value.encode_enabled} disabled={locked || !capability?.encode} onChange={(next) => onChange(direction, "encode_enabled", next)} />
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
    <Chip variant="soft" color={option.capabilities.display ? "success" : "default"}>Display：{option.capabilities.display ? "支持" : "未声明"}</Chip></div>;
}

function optionLabel(option: ListenerProtocolPackageOptionViewModel): string {
  return `${option.name} · ${option.package.id}@${option.package.version} · Schema ${option.schema.id} v${option.schema.version}`;
}

function disabledCapabilities(
  before: ScriptedSocketProcessing,
  after: SocketPayloadProcessing,
  local: boolean,
): string[] {
  if (after.mode !== "scripted") return [];
  const labels = local
    ? [["upstream", "decode_enabled", "Request Decode"], ["downstream", "encode_enabled", "Response Encode"]] as const
    : [
      ["upstream", "decode_enabled", "App → Server Decode"],
      ["upstream", "encode_enabled", "App → Server Encode"],
      ["downstream", "decode_enabled", "Server → App Decode"],
      ["downstream", "encode_enabled", "Server → App Encode"],
    ] as const;
  return labels.flatMap(([direction, field, label]) =>
    before[direction][field] && !after.settings[direction][field] ? [label] : []);
}
