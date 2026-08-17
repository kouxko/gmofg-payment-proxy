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
} from "@/generated/rust-types";
import {
  isForeignRuntimeOwner,
  runtimeOwnerModeText,
  runtimeOwnerStateText,
  runtimeOwnerTransitionText,
  type RuntimeOwnerDisplay,
} from "./android-runtime-owner-model";

interface DeviceControlCardProps {
  adb?: AndroidAdbViewModel;
  adbLoading: boolean;
  devices: AndroidDeviceViewModel[];
  devicesLoading: boolean;
  selectedSerial?: string | null;
  runtimeOwner?: RuntimeOwnerDisplay | null;
  busy: boolean;
  onRefreshDevices: () => void;
  onSelectDevice: (serial: string) => void;
  onInstall: () => void;
  onUpdate: () => void;
  onConsent: () => void;
  onRefreshStatus: () => void;
  onStop: () => void;
  onEmergencyRestore: () => void;
}

export function DeviceControlCard({
  adb,
  adbLoading,
  devices,
  devicesLoading,
  selectedSerial,
  runtimeOwner,
  busy,
  onRefreshDevices,
  onSelectDevice,
  onInstall,
  onUpdate,
  onConsent,
  onRefreshStatus,
  onStop,
  onEmergencyRestore,
}: DeviceControlCardProps): ReactElement {
  const selectedDevice = devices.find((device) => device.serial === selectedSerial);
  const foreignRuntimeOwner = isForeignRuntimeOwner(
    selectedSerial,
    runtimeOwner?.serial,
  );

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
            isDisabled={busy || !adb?.available}
            onPress={onRefreshDevices}
          >
            刷新设备列表
          </Button>
        </div>

        <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-3 max-[680px]:grid-cols-1">
          <Select
            aria-label="目标设备"
            selectedKey={selectedSerial ?? null}
            isDisabled={busy || devicesLoading || devices.length === 0}
            onSelectionChange={(serial) => {
              if (serial) onSelectDevice(String(serial));
            }}
          >
            <Label>目标设备</Label>
            <Select.Trigger>
              <Select.Value>
                {selectedDevice ? deviceDisplayName(selectedDevice) : "请选择在线设备"}
              </Select.Value>
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {devices.map((device) => (
                  <ListBox.Item
                    key={device.serial}
                    id={device.serial}
                    isDisabled={device.state !== "device"}
                    textValue={deviceDisplayName(device)}
                  >
                    <div className="flex min-w-0 flex-col">
                      <span className="truncate">{deviceDisplayName(device)}</span>
                      <span className="truncate font-mono text-xs text-[var(--telemetry-muted)]">
                        {device.serial}
                      </span>
                    </div>
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <p className="pb-2 text-xs text-[var(--telemetry-muted)]">
            {deviceStatusText(selectedSerial, devicesLoading, devices.length)}
          </p>
        </div>

        <div
          className="grid gap-1 rounded-xl border border-[var(--telemetry-line)] px-4 py-3"
          aria-label="设备网络运行所有者"
        >
          <p className="text-sm font-medium">实际运行设备</p>
          {runtimeOwner ? (
            <>
              <p className="font-mono text-xs">{runtimeOwner.serial}</p>
              <p className="text-xs text-[var(--telemetry-muted)]">
                {runtimeOwnerModeText(runtimeOwner.mode)} · {runtimeOwnerStateText(runtimeOwner)}
              </p>
              <p className="text-xs text-[var(--telemetry-muted)]">
                最近变化：{runtimeOwnerTransitionText(runtimeOwner.transition_reason)}
              </p>
              {foreignRuntimeOwner && (
                <p className="text-xs text-[var(--telemetry-warning)]">
                  当前选择的是 {selectedSerial ?? "无"}；停止、状态查询和紧急恢复只作用于实际运行设备。
                  请先停止 {runtimeOwner.serial}，再在当前选择的设备上启动或应用方案。
                </p>
              )}
            </>
          ) : (
            <p className="text-xs text-[var(--telemetry-muted)]">当前没有设备网络接管运行记录。</p>
          )}
        </div>

        <div className="grid grid-cols-6 gap-2 max-[1200px]:grid-cols-3 max-[680px]:grid-cols-1">
          <DeviceAction label="安装设备端组件" disabled={!selectedSerial || busy} onPress={onInstall} />
          <DeviceAction label="更新设备端组件" disabled={!selectedSerial || busy} onPress={onUpdate} />
          <DeviceAction label="授权网络接管" disabled={!selectedSerial || busy} onPress={onConsent} />
          <DeviceAction label="刷新运行状态" disabled={!runtimeOwner || busy} onPress={onRefreshStatus} />
          <DeviceAction label="停止网络接管" disabled={!runtimeOwner || busy} onPress={onStop} />
          <DeviceAction label="紧急恢复网络" disabled={!runtimeOwner || busy} onPress={onEmergencyRestore} danger />
        </div>
      </Card.Content>
    </Card>
  );
}

interface DeviceActionProps {
  label: string;
  disabled: boolean;
  onPress: () => void;
  danger?: boolean;
}

function DeviceAction({
  label,
  disabled,
  onPress,
  danger = false,
}: DeviceActionProps): ReactElement {
  return (
    <Button
      variant={danger ? "danger-soft" : "outline"}
      isDisabled={disabled}
      onPress={onPress}
    >
      {label}
    </Button>
  );
}

function deviceStatusText(
  selectedSerial: string | null | undefined,
  loading: boolean,
  deviceCount: number,
): string {
  if (selectedSerial) return `设备序列号：${selectedSerial}`;
  if (loading) return "正在读取设备列表…";
  if (deviceCount > 0) return `已发现 ${deviceCount} 台在线设备，请从下拉框选择。`;
  return "没有检测到在线设备";
}

function deviceDisplayName(device: AndroidDeviceViewModel): string {
  const model = device.model?.trim();
  if (!model || model.toLowerCase() === "phone") return "安卓模拟设备";
  return model;
}
