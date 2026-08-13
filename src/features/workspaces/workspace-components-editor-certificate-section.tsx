import type { ReactNode } from "react";
import { Tabs } from "@heroui/react";
import { certificateKindLabels } from "./workspace-components-editor-model";
import {
  ComponentCard,
  type WorkspaceComponentsSectionProps,
} from "./workspace-components-editor-section";

export function CertificateReferencesSection({
  workspace,
  onIntent,
  disabled,
}: WorkspaceComponentsSectionProps) {
  return (
    <Tabs.Panel id="certificates" className="space-y-3 pt-4">
      <p className="text-sm text-[var(--telemetry-muted)]">
        证书材料必须在“入口配置”中按具体用途导入。证书解析后会保存为受系统密钥保护的引用；
        Workspace 页面不创建或编辑外部文件路径。
      </p>
      {workspace.certificate_references.map((reference, index) => (
        <ComponentCard
          key={reference.id}
          title="证书引用"
          index={index}
          id={reference.id}
          disabled={disabled}
          onDelete={() => onIntent("certificate_reference", reference.id, "delete", "")}
        >
          <InfoField label="名称">{reference.label}</InfoField>
          <InfoField label="用途">{certificateKindLabels[reference.kind]}</InfoField>
          <InfoField label="保存方式" fullWidth>
            {reference.reference.startsWith("managed:listener-tls:")
              ? "系统密钥保护的 Listener TLS 引用"
              : "外部文件引用（建议在入口配置中重新导入）"}
          </InfoField>
        </ComponentCard>
      ))}
      {workspace.certificate_references.length === 0 && (
        <p className="rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3 text-sm text-[var(--telemetry-muted)]">
          当前 Workspace 尚未导入独立监听证书。
        </p>
      )}
    </Tabs.Panel>
  );
}

function InfoField({
  label,
  children,
  fullWidth,
}: {
  label: string;
  children: ReactNode;
  fullWidth?: boolean;
}) {
  return (
    <div className={fullWidth ? "col-span-2 max-[700px]:col-span-1" : ""}>
      <p className="text-xs text-[var(--telemetry-muted)]">{label}</p>
      <p>{children}</p>
    </div>
  );
}
