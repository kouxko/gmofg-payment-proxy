import type { ReactElement } from "react";
import {
  Button,
  Card,
  Chip,
  Label,
  ListBox,
  Select,
  Spinner,
} from "@heroui/react";
import type {
  AndroidAdbViewModel,
  AndroidDeviceViewModel,
  AndroidRuntimeOwnerViewModel,
} from "@/generated/rust-types";
import {
  mergeAndroidDeviceTargets,
  runtimeOwnerModeText,
  runtimeOwnerStateText,
  runtimeOwnerTransitionText,
} from "./android-runtime-owner-model";

interface DeviceControlCardProps {
  adb?: AndroidAdbViewModel;
  adbLoading: boolean;
  devices: AndroidDeviceViewModel[];
  devicesLoading: boolean;
  devicesReady: boolean;
  devicesError?: string;
  selectedSerial?: string | null;
  runtimeOwners: AndroidRuntimeOwnerViewModel[];
  busySerials: ReadonlySet<string>;
  globalBusy: boolean;
  onRefreshDevices: () => void;
  onSelectDevice: (serial: string) => void;
  onInstall: () => void;
  onUpdate: () => void;
  onConsent: () => void;
  onRefreshStatus: (owner: AndroidRuntimeOwnerViewModel) => void;
  onStop: (owner: AndroidRuntimeOwnerViewModel) => void;
  onEmergencyRestore: (owner: AndroidRuntimeOwnerViewModel) => void;
}

export function DeviceControlCard({
  adb,
  adbLoading,
  devices,
  devicesLoading,
  devicesReady,
  devicesError,
  selectedSerial,
  runtimeOwners,
  busySerials,
  globalBusy,
  onRefreshDevices,
  onSelectDevice,
  onInstall,
  onUpdate,
  onConsent,
  onRefreshStatus,
  onStop,
  onEmergencyRestore,
}: DeviceControlCardProps): ReactElement {
  const targets = mergeAndroidDeviceTargets(devices, runtimeOwners);
  const selectedTarget = targets.find((target) => target.serial === selectedSerial);
  const selectedDevice = selectedTarget?.device;
  const selectedOnline = selectedTarget?.online ?? false;
  const selectedBusy = selectedSerial ? busySerials.has(selectedSerial) : false;

  return (
    <Card className="h-fit border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>设备连接与控制</Card.Title>
        <Card.Description>通过系统 ADB 选择目标设备，并管理设备端网络接管组件。</Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4 p-4">
        <div className="flex items-center justify-between gap-3 rounded-xl bg-[var(--telemetry-soft)] px-4 py-3 max-[680px]:items-start">
          {adbLoading ? (
            <Spinner aria-label="检测安卓调试桥" />
          ) : (
            <div className="flex min-w-0 items-center gap-3">
              <Chip color={adb?.available ? "success" : "danger"} variant="soft">
                {adb?.available ? "ADB 可用" : "未找到 ADB"}
              </Chip>
              <p
                className="min-w-0 truncate text-xs text-[var(--telemetry-muted)]"
                title={adb?.executable ?? undefined}
              >
                {adb?.executable ?? "请安装安卓调试工具并加入系统路径"}
              </p>
            </div>
          )}
          <Button
            size="sm"
            variant="outline"
            isDisabled={globalBusy || !adb?.available}
            onPress={onRefreshDevices}
          >
            刷新设备列表
          </Button>
        </div>

        <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-3 max-[680px]:grid-cols-1">
          <Select
            aria-label="目标设备"
            selectedKey={selectedSerial ?? null}
            isDisabled={globalBusy || !devicesReady || targets.length === 0}
            onSelectionChange={(serial) => {
              if (serial) onSelectDevice(String(serial));
            }}
          >
            <Label>目标设备</Label>
            <Select.Trigger>
              <Select.Value>
                {!devicesReady
                  ? devicesError
                    ? "无法确认设备状态"
                    : "正在读取设备列表…"
                  : selectedDevice
                  ? deviceDisplayName(selectedDevice)
                  : selectedTarget
                    ? `离线运行设备 · ${selectedTarget.serial}`
                    : "请选择在线设备"}
              </Select.Value>
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {targets.map((target) => (
                  <ListBox.Item
                    key={target.serial}
                    id={target.serial}
                    textValue={target.device ? deviceDisplayName(target.device) : target.serial}
                  >
                    <div className="flex min-w-0 flex-col">
                      <span className="truncate">
                        {target.device ? deviceDisplayName(target.device) : "离线运行设备"}
                        {!target.online && " · 离线"}
                      </span>
                      <span className="truncate font-mono text-xs text-[var(--telemetry-muted)]">
                        {target.serial}
                      </span>
                    </div>
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <p className="pb-2 text-xs text-[var(--telemetry-muted)]">
            {deviceStatusText(
              selectedSerial,
              selectedOnline,
              devicesLoading,
              devicesReady,
              devicesError,
              devices.length,
            )}
          </p>
        </div>

        <div
          className="grid gap-1 rounded-xl border border-[var(--telemetry-line)] px-4 py-3"
          aria-label="设备网络运行所有者"
        >
          <p className="text-sm font-medium">实际运行设备</p>
          {runtimeOwners.length ? (
            <div className="grid gap-3">
              {runtimeOwners.map((owner) => {
                const online = targets.find((target) => target.serial === owner.serial)?.online ?? false;
                return (
                  <div key={`${owner.serial}:${owner.epoch}`} className="grid gap-1 rounded-lg bg-[var(--telemetry-soft)] p-3">
                    <p className="font-mono text-xs">{owner.serial}</p>
                    <p className="text-xs text-[var(--telemetry-muted)]">
                      {runtimeOwnerModeText(owner.mode)} · {runtimeOwnerStateText(owner)}
                    </p>
                    <p className="text-xs text-[var(--telemetry-muted)]">
                      最近变化：{runtimeOwnerTransitionText(owner.transition_reason)}
                    </p>
                    {!online && (
                      <p className="text-xs text-[var(--telemetry-warning)]">
                        设备离线；ADB 安装、更新和授权不可用，运行记录仍保留。
                      </p>
                    )}
                    <div className="mt-1 flex flex-wrap gap-2">
                      <DeviceAction label="刷新运行状态" accessibleSuffix={owner.serial} disabled={busySerials.has(owner.serial)} onPress={() => onRefreshStatus(owner)} />
                      <DeviceAction label="停止网络接管" accessibleSuffix={owner.serial} disabled={busySerials.has(owner.serial)} onPress={() => onStop(owner)} />
                      <DeviceAction label="紧急恢复网络" accessibleSuffix={owner.serial} disabled={busySerials.has(owner.serial)} onPress={() => onEmergencyRestore(owner)} danger />
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="text-xs text-[var(--telemetry-muted)]">当前没有设备网络接管运行记录。</p>
          )}
        </div>

        <div className="grid grid-cols-3 gap-2 max-[680px]:grid-cols-1">
          <DeviceAction label="安装设备端组件" disabled={!selectedSerial || !selectedOnline || selectedBusy} onPress={onInstall} fullWidth />
          <DeviceAction label="更新设备端组件" disabled={!selectedSerial || !selectedOnline || selectedBusy} onPress={onUpdate} fullWidth />
          <DeviceAction label="授权网络接管" disabled={!selectedSerial || !selectedOnline || selectedBusy} onPress={onConsent} fullWidth />
        </div>
        <p className="text-xs text-[var(--telemetry-muted)]">
          已保留 {runtimeOwners.length}/8 个运行设备记录。
        </p>
      </Card.Content>
    </Card>
  );
}

interface DeviceActionProps {
  label: string;
  disabled: boolean;
  onPress: () => void;
  danger?: boolean;
  accessibleSuffix?: string;
  fullWidth?: boolean;
}

function DeviceAction({
  label,
  disabled,
  onPress,
  danger = false,
  accessibleSuffix,
  fullWidth = false,
}: DeviceActionProps): ReactElement {
  return (
    <Button
      className={fullWidth ? "w-full" : undefined}
      variant={danger ? "danger-soft" : "outline"}
      aria-label={accessibleSuffix ? `${label} ${accessibleSuffix}` : undefined}
      isDisabled={disabled}
      onPress={onPress}
    >
      {label}
    </Button>
  );
}

function deviceStatusText(
  selectedSerial: string | null | undefined,
  selectedOnline: boolean,
  loading: boolean,
  ready: boolean,
  error: string | undefined,
  deviceCount: number,
): string {
  if (!ready && loading) return "正在读取设备列表…";
  if (!ready && error) return "设备列表读取失败，当前无法确认在线状态。";
  if (selectedSerial) return selectedOnline
    ? `设备序列号：${selectedSerial}`
    : `离线运行设备：${selectedSerial}`;
  if (deviceCount > 0) return `已发现 ${deviceCount} 台在线设备，请从下拉框选择。`;
  return "没有检测到在线设备";
}

function deviceDisplayName(device: AndroidDeviceViewModel): string {
  const model = device.model?.trim();
  if (!model || model.toLowerCase() === "phone") return "安卓模拟设备";
  return model;
}
