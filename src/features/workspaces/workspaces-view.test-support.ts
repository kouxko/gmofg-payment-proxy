import { vi } from "vitest";

export const mocks = {
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  workspaceCreate: vi.fn(),
  workspaceValidate: vi.fn(),
  workspaceSave: vi.fn(),
  workspaceImport: vi.fn(),
  workspaceExport: vi.fn(),
  workspaceCopy: vi.fn(),
  workspaceSelect: vi.fn(),
  workspaceDelete: vi.fn(),
  applicationConfigurationImport: vi.fn(),
  applicationConfigurationExport: vi.fn(),
  applicationBackupExport: vi.fn(),
  applicationBackupImportPrepare: vi.fn(),
  applicationBackupImportCommit: vi.fn(),
  applicationBackupImportDiscard: vi.fn(),
  legacyApplicationConfigurationImportPrepare: vi.fn(),
  legacyApplicationConfigurationImportCommit: vi.fn(),
  legacyApplicationConfigurationImportDiscard: vi.fn(),
  legacyWorkspaceImportPrepare: vi.fn(),
  legacyWorkspaceImportCommit: vi.fn(),
  legacyWorkspaceImportDiscard: vi.fn(),
  toast: vi.fn(),
};

export const workspace = {
  id: "workspace-1",
  name: "API Lab",
  revision: 1,
  listeners: [],
  metadata_extractors: [],
  response_assertions: [],
  fault_presets: [],
  certificate_references: [],
};

export const workspaceSummary = {
  id: "workspace-1",
  name: "API Lab",
  revision: 1,
  listener_count: 0,
  enabled_listener_count: 0,
  selected: true,
};

export function ok<T>(data: T) {
  return Promise.resolve({ status: "ok" as const, data });
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

export function setupWorkspaceMocks() {
  vi.clearAllMocks();
  mocks.workspaceList.mockReturnValue(ok([workspaceSummary]));
  mocks.workspaceGet.mockReturnValue(ok(workspace));
  mocks.workspaceCreate.mockImplementation((name) =>
    ok({ ...workspace, id: "workspace-2", name }),
  );
  mocks.workspaceValidate.mockImplementation((draft) =>
    ok({ valid: true, normalized: draft, field_errors: {} }),
  );
  mocks.workspaceSave.mockImplementation((draft) =>
    ok({ ...draft, revision: 2 }),
  );
  mocks.workspaceSelect.mockReturnValue(ok(workspaceSummary));
  mocks.workspaceCopy.mockReturnValue(
    ok({ ...workspace, id: "workspace-2", name: "API Lab 副本" }),
  );
  mocks.workspaceDelete.mockReturnValue(ok({ message: "Workspace 已删除。" }));
  mocks.workspaceImport.mockReturnValue(
    ok({ message: "完整 Workspace 已导入", cancelled: false }),
  );
  mocks.workspaceExport.mockReturnValue(
    ok({ message: "完整 Workspace 已导出", cancelled: false }),
  );
  mocks.applicationConfigurationImport.mockReturnValue(
    ok({
      message: "完整应用配置已导入",
      cancelled: false,
      ui_tone: "positive",
    }),
  );
  mocks.applicationConfigurationExport.mockReturnValue(
    ok({
      message: "完整应用配置已导出",
      cancelled: false,
      ui_tone: "positive",
    }),
  );
  mocks.applicationBackupExport.mockReturnValue(
    ok({ bytes_written: 2048, replaced_existing: false }),
  );
  mocks.applicationBackupImportPrepare.mockReturnValue(
    ok({
      token: "backup-token",
      expires_in_seconds: 300,
      workspace_count: 2,
      protocol_package_count: 3,
      enabled_protocol_package_count: 2,
      portable_material_count: 1,
      protocol_packages: [],
      replacement_scope: {
        replaces_all_workspaces: true,
        replaces_selected_workspace: true,
        replaces_portable_settings: true,
        replaces_protocol_package_registry: true,
      },
      migration_report: {
        removed_metadata_extractors: 0,
        source_kind: "application_configuration_document",
        source_version: 5,
      },
      warnings: [],
    }),
  );
  mocks.applicationBackupImportCommit.mockReturnValue(
    ok({
      workspace_count: 2,
      protocol_package_count: 3,
      enabled_protocol_package_count: 2,
      portable_material_count: 1,
      requires_restart: true,
    }),
  );
  mocks.applicationBackupImportDiscard.mockReturnValue(
    ok({ message: "应用备份预览已丢弃。" }),
  );
  const legacyPreview = {
    token: "legacy-token",
    expires_in_seconds: 300,
    kind: "application_configuration",
    source_version: 4,
    workspace_count: 2,
    portable_material_count: 1,
    migration_report: {
      removed_metadata_extractors: 2,
      source_kind: "application_configuration_document",
      source_version: 4,
    },
    warnings: ["2 个旧元数据提取器已移除"],
  };
  mocks.legacyApplicationConfigurationImportPrepare.mockReturnValue(ok(legacyPreview));
  mocks.legacyWorkspaceImportPrepare.mockReturnValue(
    ok({ ...legacyPreview, kind: "workspace", workspace_count: 1 }),
  );
  mocks.legacyApplicationConfigurationImportCommit.mockReturnValue(
    ok({ message: "旧版配置已导入", cancelled: false, ui_tone: "warning" }),
  );
  mocks.legacyWorkspaceImportCommit.mockReturnValue(
    ok({ message: "旧版 Workspace 已导入", cancelled: false, ui_tone: "warning" }),
  );
  mocks.legacyApplicationConfigurationImportDiscard.mockReturnValue(ok({ message: "已取消" }));
  mocks.legacyWorkspaceImportDiscard.mockReturnValue(ok({ message: "已取消" }));
}
