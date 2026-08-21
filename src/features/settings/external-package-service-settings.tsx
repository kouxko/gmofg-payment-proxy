"use client";

import {
  Alert,
  Button,
  Card,
  Chip,
  FieldError,
  Input,
  Label,
  NumberField,
  Spinner,
  TextField,
} from "@heroui/react";
import type {
  ExternalPackageServiceStatusViewModel,
  SettingsDraft,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";

type Props = {
  draft: SettingsDraft;
  fieldError: (field: string) => string | undefined;
  isDisabled: boolean;
  onDraftChange: (draft: SettingsDraft) => void;
};

export function ExternalPackageServiceSettings({
  draft,
  fieldError,
  isDisabled,
  onDraftChange,
}: Props) {
  const status = useIpcQuery<unknown>("external-package-service-status", () =>
    callCommand(commands.externalPackageServiceStatus()),
  );
  useAppEventRefresh(
    ["external_package_service_status_changed", "snapshot_required"],
    status.refresh,
  );
  const statusIsValid = isExternalPackageServiceStatus(status.data);
  const statusError = status.error ?? (!status.isLoading && !statusIsValid
    ? "外部软件包服务状态数据不完整。"
    : undefined);
  const data: ExternalPackageServiceStatusViewModel | undefined = statusIsValid
    ? status.data as ExternalPackageServiceStatusViewModel
    : undefined;
  const settings = draft.external_package_service;

  const update = (changes: Partial<typeof settings>) => onDraftChange({
    ...draft,
    external_package_service: { ...settings, ...changes },
  });

  return (
    <div className="space-y-4">
      <Alert status="warning">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>当前版本不提供连接认证</Alert.Title>
          <Alert.Description>
            外部进程通过固定的长连接路径注册并执行协议解析。绑定到非回环地址会向所在网络开放服务；请只在受信网络中使用，并通过主机防火墙限制访问。
          </Alert.Description>
        </Alert.Content>
      </Alert>

      <Card className="border border-[var(--telemetry-line)] shadow-none">
        <Card.Header className="flex-row items-start justify-between gap-3 max-[560px]:flex-col">
          <div>
            <Card.Title>外部软件包服务</Card.Title>
            <Card.Description>
              允许独立进程提供 Socket 分帧、解析、编码与协议展示能力；应用仍负责规则、入口生命周期和精确版本绑定。
            </Card.Description>
          </div>
          {data ? <ServiceStateChip data={data} /> : null}
        </Card.Header>
        <Card.Content className="space-y-4">
          {status.isLoading ? <Spinner aria-label="正在读取外部软件包服务状态" /> : null}
          {statusError ? (
            <Alert status="danger">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>无法读取外部软件包服务状态</Alert.Title>
                <Alert.Description>{statusError}</Alert.Description>
              </Alert.Content>
              <Button size="sm" variant="outline" onPress={() => void status.refresh()}>重试</Button>
            </Alert>
          ) : null}
          {data ? (
            <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm max-[560px]:grid-cols-1">
              <dt className="text-[var(--telemetry-muted)]">实际监听地址</dt>
              <dd className="break-all font-mono">{data.websocket_url}</dd>
              <dt className="text-[var(--telemetry-muted)]">固定路径</dt>
              <dd className="font-mono">{data.fixed_path}</dd>
              <dt className="text-[var(--telemetry-muted)]">在线连接</dt>
              <dd>{data.online_connection_count} 个</dd>
              <dt className="text-[var(--telemetry-muted)]">认证</dt>
              <dd>{data.authentication_enabled ? "已启用" : "未启用"}</dd>
              {data.state.state === "failed" ? <>
                <dt className="text-[var(--telemetry-muted)]">启动错误</dt>
                <dd className="break-words text-danger">{data.state.error}</dd>
              </> : null}
            </dl>
          ) : null}
        </Card.Content>
      </Card>

      <div className="grid grid-cols-2 gap-4 max-[760px]:grid-cols-1">
        <TextField
          isInvalid={fieldError("external_package_service.bind_address") != null}
          isDisabled={isDisabled}
          value={settings.bind_address}
          onChange={(bind_address) => update({ bind_address })}
        >
          <Label>监听地址</Label>
          <Input className="w-full" />
          {fieldError("external_package_service.bind_address") ? (
            <FieldError>{fieldError("external_package_service.bind_address")}</FieldError>
          ) : null}
        </TextField>
        <ServiceNumberField label="端口" value={settings.port} minValue={1} maxValue={65535}
          error={fieldError("external_package_service.port")} isDisabled={isDisabled}
          onChange={(port) => update({ port })} />
        <ServiceNumberField label="RPC 超时（秒）" value={settings.rpc_timeout_seconds} minValue={1} maxValue={300}
          error={fieldError("external_package_service.rpc_timeout_seconds")} isDisabled={isDisabled}
          onChange={(rpc_timeout_seconds) => update({ rpc_timeout_seconds })} />
        <ServiceNumberField label="最大并发 RPC" value={settings.max_in_flight} minValue={1}
          error={fieldError("external_package_service.max_in_flight")} isDisabled={isDisabled}
          onChange={(max_in_flight) => update({ max_in_flight })} />
      </div>

      <Alert status="accent">
        保存只会持久化下次启动配置。实际监听地址、已建立连接和运行中的入口不会在当前进程内切换；重启应用后生效。
      </Alert>
    </div>
  );
}

function ServiceNumberField({ label, value, minValue, maxValue, error, isDisabled, onChange }: {
  label: string;
  value: number;
  minValue: number;
  maxValue?: number;
  error?: string;
  isDisabled: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <NumberField value={value} minValue={minValue} maxValue={maxValue}
      isInvalid={error != null} isDisabled={isDisabled} onChange={onChange}>
      <Label>{label}</Label>
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
      {error ? <FieldError>{error}</FieldError> : null}
    </NumberField>
  );
}

function ServiceStateChip({ data }: { data: ExternalPackageServiceStatusViewModel }) {
  return data.state.state === "listening"
    ? <Chip color="success" variant="soft">正在监听</Chip>
    : <Chip color="danger" variant="soft">启动失败</Chip>;
}

/**
 * IPC 运行时边界严格验证完整快照，拒绝旧 Host、额外字段和畸形计数。设置页不会用草稿
 * 地址伪装实际监听状态，也不会在缺失认证字段时默认显示为安全。
 */
export function isExternalPackageServiceStatus(
  value: unknown,
): value is ExternalPackageServiceStatusViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["websocket_url", "fixed_path", "online_connection_count", "state", "authentication_enabled"])
    || typeof value.websocket_url !== "string"
    || !value.websocket_url.startsWith("ws://")
    || value.fixed_path !== "/packages"
    || !Number.isSafeInteger(value.online_connection_count)
    || Number(value.online_connection_count) < 0
    || typeof value.authentication_enabled !== "boolean"
    || !isRecord(value.state)) return false;
  return (value.state.state === "listening" && hasOnly(value.state, ["state"]))
    || (value.state.state === "failed"
      && hasOnly(value.state, ["state", "error"])
      && typeof value.state.error === "string"
      && value.state.error.length > 0);
}

function hasOnly(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => actual.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
