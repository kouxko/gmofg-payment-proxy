import type {
  ProtocolPackageDetailViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageImportPreviewViewModel,
  ProtocolPackageImportViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";

export function version(
  value: string,
  overrides: Partial<ProtocolPackageVersionViewModel> = {},
): ProtocolPackageVersionViewModel {
  return {
    package: { id: "iso-8583", version: value },
    name: "ISO 8583 长名称协议包",
    host_api: 1,
    built_in: false,
    enabled: value === "2.0.0",
    validation: { state: "valid" },
    installed_at: "2026-08-14T08:00:00Z",
    ...overrides,
  };
}

export function group(
  overrides: Partial<ProtocolPackageGroupViewModel> = {},
): ProtocolPackageGroupViewModel {
  return {
    id: "iso-8583",
    name: "ISO 8583",
    // Application 按 Rust SemVer 从旧到新返回；UI 只负责反转展示顺序。
    versions: [version("1.2.0"), version("1.10.0"), version("2.0.0")],
    reference_count: 3,
    active_reference_count: 1,
    ...overrides,
  };
}

export function detail(
  selected = version("2.0.0"),
  overrides: Partial<ProtocolPackageDetailViewModel> = {},
): ProtocolPackageDetailViewModel {
  return {
    version: selected,
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: false },
      display: true,
    },
    schema: {
      id: "iso-message",
      version: 1,
      title: "ISO 8583 消息",
      fields: [
        { name: "message_type", label: "消息类型", type: "string" },
        { name: "amount", label: "交易金额", type: "int" },
      ],
    },
    usages: [
      {
        workspace_id: "workspace-1",
        workspace_name: "收银台测试",
        listener_id: "listener-1",
        listener_name: "上游 Socket",
        listener_enabled: true,
        runtime_state: "running",
      },
    ],
    ...overrides,
  };
}

export function importPreview(
  overrides: Partial<ProtocolPackageImportPreviewViewModel> = {},
): ProtocolPackageImportPreviewViewModel {
  return {
    token: "018f-import-token",
    disposition: "new",
    package: { id: "iso-8583", version: "3.0.0" },
    name: "ISO 8583 导入包",
    host_api: 1,
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: false },
      display: true,
    },
    schema: {
      id: "iso-message",
      version: 2,
      title: "ISO 导入 Schema",
      fields: [
        { name: "mti", label: "消息类型", type: "string" },
        { name: "amount", label: "金额", type: "int" },
      ],
    },
    ...overrides,
  };
}

export function importResult(
  outcome: ProtocolPackageImportViewModel["outcome"] = "installed",
): ProtocolPackageImportViewModel {
  const preview = importPreview();
  return {
    outcome,
    version: version("3.0.0", {
      package: preview.package,
      name: preview.name,
      enabled: false,
    }),
    capabilities: preview.capabilities,
    schema: preview.schema,
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
