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
    kind: "socket",
    package_source: { type: "external", online: true },
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
    kind: "socket",
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
  const external = selected.package_source.type === "external" ? externalDetail() : null;
  return {
    version: selected,
    kind: "socket",
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    upstream_schema: {
      root: { type: "object", title: "ISO 8583 消息", properties: {
        message_type: { type: "string", title: "消息类型" },
        amount: { type: "number", title: "交易金额" },
      } },
    },
    downstream_schema: {
      root: { type: "object", title: "ISO 8583 响应", properties: {
        response_code: { type: "string", title: "响应码" },
      } },
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
    external,
    ...overrides,
  };
}

function externalDetail(): NonNullable<ProtocolPackageDetailViewModel["external"]> {
  return {
    remote_address: "127.0.0.1:49152",
    connection_id: "018f6fc0-65d8-7d90-b25b-392f6d9b9481",
    first_connected_at: "2026-08-20T08:00:00Z",
    last_connected_at: "2026-08-20T09:00:00Z",
    registration_fingerprint_sha256: "ab".repeat(32),
    upstream_methods: externalMethods("upstream"),
    downstream_methods: externalMethods("downstream"),
    recent_error: null,
  };
}

function externalMethods(direction: "upstream" | "downstream") {
  return {
    frame: `hooks.${direction}.frame`,
    decode: `hooks.${direction}.decode`,
    encode: `hooks.${direction}.encode`,
    display: `document.${direction}.display`,
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
    kind: "socket",
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    upstream_schema: {
      root: { type: "object", title: "ISO 导入 Schema", properties: {
        mti: { type: "string", title: "消息类型" },
        amount: { type: "number", title: "金额" },
      } },
    },
    downstream_schema: {
      root: { type: "object", title: "ISO 导入响应 Schema", properties: {
        response_code: { type: "string", title: "响应码" },
      } },
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
    kind: preview.kind,
    capabilities: preview.capabilities,
    upstream_schema: preview.upstream_schema,
    downstream_schema: preview.downstream_schema,
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
