import { vi } from "vitest";

export const mocks = {
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  workspaceCreate: vi.fn(),
  workspaceValidate: vi.fn(),
  workspaceSave: vi.fn(),
  workspaceCopy: vi.fn(),
  workspaceSelect: vi.fn(),
  workspaceDelete: vi.fn(),
  applicationBackupExport: vi.fn(),
  applicationBackupImportPrepare: vi.fn(),
  applicationBackupImportCommit: vi.fn(),
  applicationBackupImportDiscard: vi.fn(),
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
}
