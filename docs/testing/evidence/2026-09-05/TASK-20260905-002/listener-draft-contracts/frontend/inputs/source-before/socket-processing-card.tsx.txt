import { Alert, Button, Card, Chip, Label, ListBox, Select, Spinner } from "@heroui/react";
import { useState, type Key } from "react";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { bindPackage, exactPackageKey, matchingOption, setProcessingMode, socketCatalogOptions } from "./socket-listener-model";
import { SocketProtocolPackageDialog } from "./socket-protocol-package-dialog";

export interface ProtocolCatalogState {
  data?: ListenerProtocolPackageCatalogViewModel;
  error?: string;
  loading: boolean;
  refresh: () => Promise<void>;
}

const TRANSPARENT_RELAY_KEY = "__transparent_relay__";

export function SocketProcessingCard({ settings, catalog, locked, onChange }: {
  settings: SocketRelaySettings;
  catalog: ProtocolCatalogState;
  locked: boolean;
  onChange: (settings: SocketRelaySettings) => void;
}) {
  // 包切换可能按能力原子关闭开关，必须通过 live region 告知键盘/读屏用户。
  const [announcement, setAnnouncement] = useState("");
  const processing = settings.processing;
  const scripted = processing.mode === "scripted" ? processing.settings : undefined;
  const local = settings.topology.mode === "local_responder";
  // useIpcQuery 刷新时会保留旧 data。加载或错误状态下必须把旧快照视为不可用，
  // 否则用户可能在 Rust 正在重验/已经拒绝目录时继续修改方向开关。
  const selected = catalog.loading || catalog.error
    ? undefined
    : scripted ? matchingOption(catalog.data, scripted.package) : undefined;
  const selectedKey = scripted ? exactPackageKey(scripted.package) : TRANSPARENT_RELAY_KEY;
  const hasBoundPackage = Boolean(scripted && scripted.package.id.length > 0 && scripted.package.version.length > 0);
  const missingFromCatalog = hasBoundPackage && !selected;
  const unavailableBound = missingFromCatalog
    && !catalog.loading
    && !catalog.error
    && Boolean(catalog.data);
  const socketOptions = socketCatalogOptions(catalog.data);

  function selectPackage(key: Key | null) {
    if (!local && key === TRANSPARENT_RELAY_KEY) {
      onChange(setProcessingMode(settings, "direct"));
      setAnnouncement("已取消协议包；数据将保持原样透明转发。");
      return;
    }
    const option = socketOptions.find((item) => exactPackageKey(item.package) === key);
    if (!option) return;
    const current = processing.mode === "scripted"
      ? processing
      : setProcessingMode(settings, "scripted").processing;
    if (current.mode !== "scripted") return;
    const nextProcessing = bindPackage(current, option, local);
    setAnnouncement(`已选择 ${option.name}；完整的分帧、解析、规则、编码和显示处理链将自动应用。`);
    onChange({ ...settings, processing: nextProcessing });
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>4. 协议处理</Card.Title>
        <Card.Description>{local
          ? "选择用于解析请求并生成本机应答的协议包。"
          : "不选择协议包时透明转发；选择协议包后可按字段匹配和改写数据。"}</Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4">
        {catalog.loading && <Spinner aria-label="正在读取入口协议包目录" />}
        {catalog.error && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>协议包目录读取失败</Alert.Title><Alert.Description>{catalog.error}</Alert.Description>
          <Button size="sm" variant="outline" onPress={() => void catalog.refresh()}>重试</Button>
        </Alert.Content></Alert>}
        {local && !hasBoundPackage && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>尚未选择处理方案</Alert.Title>
          <Alert.Description>请选择一个可用的处理方案后再配置处理步骤。</Alert.Description>
        </Alert.Content></Alert>}
        {!catalog.loading && !catalog.error && socketOptions.length === 0 && (
          <Alert status="warning"><Alert.Indicator /><Alert.Content>
            <Alert.Title>没有可绑定的 Socket 协议包版本</Alert.Title>
            <Alert.Description>
              已安装 {catalog.data?.installed_version_count ?? 0} 个版本，其中 {catalog.data?.unavailable_version_count ?? 0} 个当前不可用。
              HTTP 协议包不会出现在此处；请先在协议包页面导入、修复或启用兼容的 Socket 版本。
            </Alert.Description>
          </Alert.Content></Alert>
        )}
        {unavailableBound && <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>当前处理方案已不可用</Alert.Title>
          <Alert.Description>
            精确身份 {scripted?.package.id}@{scripted?.package.version} 仍会保留，不会自动替换。
            该版本可能已停用、校验失败，或其外部进程已离线；恢复可用后刷新目录，或选择新的方案。
          </Alert.Description>
        </Alert.Content></Alert>}
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
          <Select aria-label="Socket 协议处理方案" selectedKey={selectedKey}
            isDisabled={locked || (local && (catalog.loading || Boolean(catalog.error) || socketOptions.length === 0))}
            onSelectionChange={selectPackage}>
            <Label>协议包</Label>
            <Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              {!local && <ListBox.Item id={TRANSPARENT_RELAY_KEY} textValue="不使用协议包（透明转发）">
                不使用协议包（透明转发）
              </ListBox.Item>}
              {missingFromCatalog && <ListBox.Item id={selectedKey} isDisabled textValue="当前选择（不可用）">
                {scripted?.package.id}@{scripted?.package.version} · 当前选择（不可用）
              </ListBox.Item>}
              {(!catalog.loading && !catalog.error ? socketOptions : []).map((option) => (
                <ListBox.Item key={exactPackageKey(option.package)} id={exactPackageKey(option.package)} textValue={optionLabel(option)}>
                  {optionLabel(option)}
                </ListBox.Item>
              ))}
            </ListBox></Select.Popover>
          </Select>
        </div>
        {scripted && hasBoundPackage && <details className="rounded-xl border border-[var(--telemetry-line)] p-3">
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
        {!scripted && !local && <p className="text-sm text-[var(--telemetry-muted)]">
          当前未使用协议包，应用与上游之间的数据保持原样转发。
        </p>}
        {scripted && selected && <p className="text-sm text-[var(--telemetry-muted)]">
          {local
            ? "应用请求会自动解析为字段，规则处理后自动编码为本机应答；协议视图按包声明生成。"
            : "双向数据会自动解析为字段，规则处理后按包能力重新编码；协议视图按包声明生成。"}
        </p>}
      </Card.Content>
    </Card>
  );
}

function PackageSummary({ option }: { option: ListenerProtocolPackageOptionViewModel }) {
  return <div className="flex flex-wrap gap-2 text-sm"><Chip variant="soft">{option.package.id}@{option.package.version}</Chip>
    <Chip variant="soft" color={option.package_source.type === "external" ? "warning" : "accent"}>{sourceLabel(option)}</Chip>
    <Chip variant="soft">上行字段结构 {option.upstream_schema?.root.title || "未命名 Schema"}</Chip>
    <Chip variant="soft">下行字段结构 {option.downstream_schema?.root.title || "未命名 Schema"}</Chip>
    <Chip variant="soft">报文边界与字段解析：双向支持</Chip>
    <Chip variant="soft">报文重建：上行 {option.capabilities.upstream.encode ? "支持" : "不支持"}，下行 {option.capabilities.downstream.encode ? "支持" : "不支持"}</Chip>
    <Chip variant="soft" color={option.capabilities.display ? "success" : "default"}>协议视图：{option.capabilities.display ? "支持" : "未提供"}</Chip></div>;
}

function optionLabel(option: ListenerProtocolPackageOptionViewModel): string {
  return `${option.name} · ${option.package.version} · ${sourceLabel(option)}`;
}

function sourceLabel(option: ListenerProtocolPackageOptionViewModel): string {
  if (option.package_source.type === "managed") {
    return option.package_source.online ? "本地管理 · 运行中" : "本地管理 · 已停止";
  }
  return option.package_source.online ? "远端调试 · 在线" : "远端调试 · 离线";
}
