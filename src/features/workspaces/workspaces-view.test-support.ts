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
}
