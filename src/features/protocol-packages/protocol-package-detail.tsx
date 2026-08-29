import { Alert, Button, Chip, Spinner, Table } from "@heroui/react";
import type {
  ListenerRuntimeState,
  ProtocolPackageDetailViewModel,
} from "@/generated/rust-types";
import { flattenSchema, schemaTitle } from "@/lib/protocol-package-schema";
import {
  capabilityItems,
  isBuiltInPackage,
  packageSourceText,
  protocolPackageKindText,
  validationText,
} from "./protocol-package-model";

interface DetailState {
  data?: ProtocolPackageDetailViewModel;
  error?: string;
  isLoading: boolean;
}

export function ProtocolPackageDetail({
  detail,
  enablePending = false,
  enableError,
  disablePending = false,
  disableError,
  deleteBlockedReason,
  onEnable,
  onDisable,
  onRequestDelete,
}: {
  detail: DetailState;
  enablePending?: boolean;
  enableError?: string;
  disablePending?: boolean;
  disableError?: string;
  deleteBlockedReason?: string;
  onEnable?: () => void;
  onDisable?: () => void;
  onRequestDelete?: () => void;
}) {
  if (detail.isLoading) {
    return <div className="grid min-h-56 place-items-center"><Spinner aria-label="正在读取协议包详情" /></div>;
  }
  if (detail.error) {
    return (
      <Alert status="danger">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>协议包详情读取失败</Alert.Title>
          <Alert.Description>{detail.error}</Alert.Description>
        </Alert.Content>
      </Alert>
    );
  }
  if (!detail.data) {
    return <p className="p-6 text-center text-sm text-[var(--telemetry-muted)]">选择一个版本查看详情。</p>;
  }

  const { version, kind, capabilities, upstream_schema, downstream_schema, usages, external } = detail.data;
  return (
    <div className="min-w-0 space-y-5">
      <section aria-labelledby="package-identity-heading">
        <h3 id="package-identity-heading" className="mb-2 font-semibold">身份与校验</h3>
        <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1">
          <dt className="text-[var(--telemetry-muted)]">名称</dt><dd className="min-w-0 break-words">{version.name || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">包 ID</dt><dd className="min-w-0 break-all font-mono">{version.package.id || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">版本</dt><dd className="font-mono">{version.package.version || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">Host API</dt><dd>{version.host_api}</dd>
          <dt className="text-[var(--telemetry-muted)]">适用协议</dt><dd>{protocolPackageKindText(kind)}</dd>
          <dt className="text-[var(--telemetry-muted)]">来源</dt><dd>{packageSourceText(version)}</dd>
          <dt className="text-[var(--telemetry-muted)]">校验</dt><dd>{validationText(version.validation)}</dd>
          <dt className="text-[var(--telemetry-muted)]">状态</dt><dd>{version.enabled ? "已启用" : "已停用"}</dd>
          <dt className="text-[var(--telemetry-muted)]">安装时间</dt><dd className="break-all">{version.installed_at || "—"}</dd>
        </dl>
        {!version.enabled && version.validation.state === "valid" && onEnable ? (
          <div className="mt-4 space-y-3">
            {enableError ? (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>协议包启用失败</Alert.Title>
                  <Alert.Description>{enableError}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : null}
            <Button
              variant="primary"
              isDisabled={enablePending || (version.package_source.type === "external" && !version.package_source.online)}
              onPress={onEnable}
            >
              {enablePending ? "正在启用…" : "启用协议包"}
            </Button>
            <p className="text-sm text-[var(--telemetry-muted)]">
              {version.package_source.type === "external" && !version.package_source.online
                ? "外部软件包离线，重新连接并完成注册后才能启用。"
                : "启用后可在匹配的入口配置中选择此版本。"}
            </p>
          </div>
        ) : null}
        {version.package_source.type === "external" ? (
          <div className="mt-4 space-y-3 rounded-xl border border-[var(--telemetry-line)] p-4">
            <h4 className="font-medium">外部软件包生命周期</h4>
            {disableError ? (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>外部软件包停用失败</Alert.Title>
                  <Alert.Description>{disableError}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : null}
            <div className="flex flex-wrap gap-2">
              {version.enabled && onDisable ? (
                <Button variant="outline" isDisabled={disablePending} onPress={onDisable}>
                  {disablePending ? "正在停用…" : "停用外部软件包"}
                </Button>
              ) : null}
              {onRequestDelete ? (
                <Button variant="danger" isDisabled={disablePending || deleteBlockedReason !== undefined} onPress={onRequestDelete}>
                  删除外部软件包
                </Button>
              ) : null}
            </div>
            {deleteBlockedReason ? (
              <Alert status="warning">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>仍有 {usages.length} 个入口引用此精确版本，不能删除。</Alert.Title>
                  <Alert.Description>{deleteBlockedReason}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : (
              <p className="text-sm text-[var(--telemetry-muted)]">
                删除会移除此精确版本的元数据；若软件包仍在线，Proxy 会先关闭对应连接。
              </p>
            )}
          </div>
        ) : null}
      </section>

      {external ? (
        <section aria-labelledby="external-package-connection-heading">
          <h3 id="external-package-connection-heading" className="mb-2 font-semibold">外部连接</h3>
          <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1">
            <dt className="text-[var(--telemetry-muted)]">远端地址</dt><dd className="break-all font-mono">{external.remote_address ?? "离线"}</dd>
            <dt className="text-[var(--telemetry-muted)]">连接 ID</dt><dd className="break-all font-mono">{external.connection_id ?? "离线"}</dd>
            <dt className="text-[var(--telemetry-muted)]">首次连接</dt><dd className="break-all">{external.first_connected_at}</dd>
            <dt className="text-[var(--telemetry-muted)]">最近连接</dt><dd className="break-all">{external.last_connected_at}</dd>
            <dt className="text-[var(--telemetry-muted)]">注册指纹</dt><dd className="break-all font-mono text-xs">{external.registration_fingerprint_sha256}</dd>
            <dt className="text-[var(--telemetry-muted)]">RPC 超时</dt><dd>{external.rpc_timeout_seconds} 秒</dd>
            <dt className="text-[var(--telemetry-muted)]">上行方法</dt><dd className="break-all font-mono text-xs">{methodSummary(external.upstream_methods)}</dd>
            <dt className="text-[var(--telemetry-muted)]">下行方法</dt><dd className="break-all font-mono text-xs">{methodSummary(external.downstream_methods)}</dd>
          </dl>
          {external.recent_error ? (
            <Alert status="danger" className="mt-3">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>最近错误：{external.recent_error.code}</Alert.Title>
                <Alert.Description>{external.recent_error.message} · {external.recent_error.occurred_at}</Alert.Description>
              </Alert.Content>
            </Alert>
          ) : null}
        </section>
      ) : null}

      {isBuiltInPackage(version) && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>ISO 8583:1987 ASCII Profile</Alert.Title>
            <Alert.Description>
              模板覆盖主位图、次位图和 DE2–DE128 字段结构。2 字节大端长度头属于当前 Socket 传输约定；接入真实系统前，仍需按对端的字段编码和私有域规格调整。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}

      <section aria-labelledby="package-capabilities-heading">
        <h3 id="package-capabilities-heading" className="mb-2 font-semibold">能力</h3>
        <div className="flex flex-wrap gap-2">
          {capabilityItems(capabilities).map(([label, supported]) => (
            <Chip key={label} size="sm" color={supported ? "success" : "default"} variant="soft">
              {label}：{supported ? "支持" : "不支持"}
            </Chip>
          ))}
        </div>
      </section>

      <section aria-labelledby="package-usages-heading">
        <h3 id="package-usages-heading" className="mb-2 font-semibold">使用者</h3>
        {usages.length === 0 ? (
          <p className="text-sm text-[var(--telemetry-muted)]">当前没有入口引用此版本。</p>
        ) : (
          <ul className="space-y-2 text-sm">
            {usages.map((usage) => (
              <li key={`${usage.workspace_id}:${usage.listener_id}`} className="min-w-0 rounded-lg bg-[var(--telemetry-table-head)] p-3">
                <span className="block break-words font-medium">{usage.workspace_name || "未命名工作区"} / {usage.listener_name || "未命名入口"}</span>
                <span className="block break-all font-mono text-xs text-[var(--telemetry-muted)]">
                  {usage.workspace_id} / {usage.listener_id}
                </span>
                <span className="block text-[var(--telemetry-muted)]">
                  {usage.listener_enabled ? "已启用" : "已停用"} · {runtimeStateText(usage.runtime_state)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <SchemaSection id="package-upstream-schema" title={kind === "http" ? "请求 Schema" : "上行 Schema"} schema={upstream_schema} />
      <SchemaSection id="package-downstream-schema" title={kind === "http" ? "响应 Schema" : "下行 Schema"} schema={downstream_schema} />
    </div>
  );
}

function methodSummary(methods: NonNullable<ProtocolPackageDetailViewModel["external"]>["upstream_methods"]): string {
  return `frame=${methods.frame} · decode=${methods.decode} · encode=${methods.encode} · display=${methods.display}`;
}

function SchemaSection({ id, title, schema }: {
  id: string;
  title: string;
  schema: ProtocolPackageDetailViewModel["upstream_schema"];
}) {
  return (
    <section aria-labelledby={`${id}-heading`}>
      <h3 id={`${id}-heading`} className="mb-2 font-semibold">{title}</h3>
      <p className="mb-2 min-w-0 break-words text-sm text-[var(--telemetry-muted)]">
        {schemaTitle(schema)}
      </p>
      <Table>
        <Table.ScrollContainer>
          <Table.Content aria-label={`${title} 字段`} className="min-w-[520px]">
            <Table.Header>
              <Table.Column isRowHeader>字段名</Table.Column>
              <Table.Column>标签</Table.Column>
              <Table.Column>类型</Table.Column>
            </Table.Header>
            <Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">此 Schema 没有声明字段。</div>}>
              {flattenSchema(schema.root).map((node, index) => (
                <Table.Row key={`${node.path}:${index}`} id={`${id}:${node.path}:${index}`}>
                  <Table.Cell className="max-w-64 break-all font-mono text-xs">{node.path}</Table.Cell>
                  <Table.Cell className="max-w-80 break-words">{node.title || "—"}</Table.Cell>
                  <Table.Cell className="font-mono text-xs">{node.type}</Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
      </Table>
    </section>
  );
}

function runtimeStateText(state: ListenerRuntimeState): string {
  const labels: Record<ListenerRuntimeState, string> = {
    stopped: "已停止",
    starting: "启动中",
    running: "运行中",
    stopping: "停止中",
    faulted: "故障",
  };
  return labels[state];
}
