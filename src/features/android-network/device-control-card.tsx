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

interface DeviceControlCardProps {
  adb?: AndroidAdbViewModel;
  adbLoading: boolean;
  devices: AndroidDeviceViewModel[];
  devicesLoading: boolean;
  selectedSerial?: string | null;
  busy: boolean;
  onRefreshDevices: () => void;
  onSelectDevice: (serial: string) => void;
  onInstall: () => void;
  onUpdate: () => void;
  onConsent: () => void;
  onRefreshStatus: () => void;
  onEmergencyRestore: () => void;
}

export function DeviceControlCard({
  adb,
  adbLoading,
  devices,
  devicesLoading,
  selectedSerial,
  busy,
  onRefreshDevices,
  onSelectDevice,
  onInstall,
  onUpdate,
  onConsent,
  onRefreshStatus,
  onEmergencyRestore,
}: DeviceControlCardProps): ReactElement {
  const selectedDevice = devices.find((device) => device.serial === selectedSerial);

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

        <div className="grid grid-cols-5 gap-2 max-[1100px]:grid-cols-3 max-[680px]:grid-cols-1">
          <DeviceAction label="安装设备端组件" disabled={!selectedSerial || busy} onPress={onInstall} />
          <DeviceAction label="更新设备端组件" disabled={!selectedSerial || busy} onPress={onUpdate} />
          <DeviceAction label="授权网络接管" disabled={!selectedSerial || busy} onPress={onConsent} />
          <DeviceAction label="刷新运行状态" disabled={!selectedSerial || busy} onPress={onRefreshStatus} />
          <DeviceAction label="紧急恢复网络" disabled={!selectedSerial || busy} onPress={onEmergencyRestore} danger />
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
