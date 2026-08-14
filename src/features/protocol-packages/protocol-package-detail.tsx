import { Alert, Chip, Spinner, Table } from "@heroui/react";
import type {
  ListenerRuntimeState,
  ProtocolPackageDetailViewModel,
} from "@/generated/rust-types";
import { capabilityItems, validationText } from "./protocol-package-model";

interface DetailState {
  data?: ProtocolPackageDetailViewModel;
  error?: string;
  isLoading: boolean;
}

export function ProtocolPackageDetail({ detail }: { detail: DetailState }) {
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

  const { version, capabilities, schema, usages } = detail.data;
  const fields = schema.fields;
  return (
    <div className="min-w-0 space-y-5">
      <section aria-labelledby="package-identity-heading">
        <h3 id="package-identity-heading" className="mb-2 font-semibold">身份与校验</h3>
        <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1">
          <dt className="text-[var(--telemetry-muted)]">名称</dt><dd className="min-w-0 break-words">{version.name || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">包 ID</dt><dd className="min-w-0 break-all font-mono">{version.package.id || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">版本</dt><dd className="font-mono">{version.package.version || "—"}</dd>
          <dt className="text-[var(--telemetry-muted)]">Host API</dt><dd>{version.host_api}</dd>
          <dt className="text-[var(--telemetry-muted)]">校验</dt><dd>{validationText(version.validation)}</dd>
          <dt className="text-[var(--telemetry-muted)]">状态</dt><dd>{version.enabled ? "已启用" : "已停用"}</dd>
          <dt className="text-[var(--telemetry-muted)]">安装时间</dt><dd className="break-all">{version.installed_at || "—"}</dd>
        </dl>
      </section>

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
          <p className="text-sm text-[var(--telemetry-muted)]">当前没有 Listener 引用此版本。</p>
        ) : (
          <ul className="space-y-2 text-sm">
            {usages.map((usage) => (
              <li key={`${usage.workspace_id}:${usage.listener_id}`} className="min-w-0 rounded-lg bg-[var(--telemetry-table-head)] p-3">
                <span className="block break-words font-medium">{usage.workspace_name || "未命名 Workspace"} / {usage.listener_name || "未命名 Listener"}</span>
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

      <section aria-labelledby="package-schema-heading">
        <h3 id="package-schema-heading" className="mb-2 font-semibold">Schema</h3>
        <p className="mb-2 min-w-0 break-words text-sm text-[var(--telemetry-muted)]">
          {schema.title || "未命名 Schema"} · <span className="font-mono">{schema.id || "—"}</span> · v{schema.version}
        </p>
        <Table>
          <Table.ScrollContainer>
            <Table.Content aria-label="协议包 Schema 字段" className="min-w-[520px]">
              <Table.Header>
                <Table.Column isRowHeader>字段名</Table.Column>
                <Table.Column>标签</Table.Column>
                <Table.Column>类型</Table.Column>
              </Table.Header>
              <Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">此 Schema 没有声明字段。</div>}>
                {fields.map((field, index) => (
                  <Table.Row key={`${field.name}:${index}`} id={`${field.name}:${index}`}>
                    <Table.Cell className="max-w-64 break-all font-mono text-xs">{field.name || "—"}</Table.Cell>
                    <Table.Cell className="max-w-80 break-words">{field.label || "—"}</Table.Cell>
                    <Table.Cell className="font-mono text-xs">{field.type || "—"}</Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      </section>
    </div>
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
