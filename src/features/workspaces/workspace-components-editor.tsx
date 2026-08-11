"use client";

/**
 * Workspace 中可复用策略的纯展示编辑器。
 *
 * 这里仅把用户输入投影回 Rust 生成的 DTO，不生成领域 ID、不读取文件、不解析证书，
 * 也不执行 JSONPath、编解码或断言。新增组件必须调用 Rust 的
 * `workspace_component_new`，最终合法性统一由 `workspace_validate` 决定。
 */

import { Tabs } from "@heroui/react";
import type { ProxyWorkspace } from "@/generated/rust-types";
import { CertificateReferencesSection } from "./workspace-components-editor-certificate-section";
import { FaultPresetsSection } from "./workspace-components-editor-fault-section";
import { MetadataExtractorsSection } from "./workspace-components-editor-metadata-section";
import type {
  ComponentKind,
  ComponentOperation,
} from "./workspace-components-editor-model";
import { ResponseAssertionsSection } from "./workspace-components-editor-response-section";

export function WorkspaceComponentsEditor({
  workspace,
  onChange,
  onAdd,
  onIntent,
  disabled,
}: {
  workspace: ProxyWorkspace;
  onChange: (workspace: ProxyWorkspace) => void;
  onAdd: (kind: ComponentKind) => void;
  onIntent: (
    kind: ComponentKind,
    id: string,
    operation: ComponentOperation,
    value: string,
  ) => void;
  disabled: boolean;
}) {
  const sectionProps = { workspace, onChange, onAdd, onIntent, disabled };

  return (
    <Tabs aria-label="Workspace 策略配置" defaultSelectedKey="extractors">
      <Tabs.ListContainer>
        <Tabs.List>
          <Tabs.Tab id="extractors">元数据提取</Tabs.Tab>
          <Tabs.Tab id="assertions">响应断言</Tabs.Tab>
          <Tabs.Tab id="certificates">证书引用</Tabs.Tab>
          <Tabs.Tab id="faults">连接故障预设</Tabs.Tab>
        </Tabs.List>
      </Tabs.ListContainer>

      <MetadataExtractorsSection {...sectionProps} />
      <ResponseAssertionsSection {...sectionProps} />
      <CertificateReferencesSection {...sectionProps} />
      <FaultPresetsSection {...sectionProps} />
    </Tabs>
  );
}
