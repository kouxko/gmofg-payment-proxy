import type {
  ProtocolPackageCapabilitiesViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageRef,
  ProtocolPackageValidationViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { BUILT_IN_ISO_8583_PACKAGE } from "@/lib/protocol-package-identity";
import { isProtocolPackageSchema } from "@/lib/protocol-package-schema";
import {
  isBuiltInPackage,
  isProtocolPackageSource,
} from "./protocol-package-source";

export {
  isBuiltInPackage,
  isExternalPackage,
  isManagedPackage,
  packageSourceText,
} from "./protocol-package-source";

export function builtInRestoreResultError(value: unknown): string | undefined {
  if (!isRecord(value)
    || !hasOnly(value, ["outcome", "version", "kind", "capabilities", "upstream_schema", "downstream_schema"])
    || (value.outcome !== "installed" && value.outcome !== "reused")
    || !isProtocolPackageVersion(value.version)
    || !isBuiltInPackage(value.version)
    || value.version.enabled !== true
    || value.version.validation.state !== "valid"
    || value.version.package.id !== BUILT_IN_ISO_8583_PACKAGE.id
    || value.version.package.version !== BUILT_IN_ISO_8583_PACKAGE.version
    || value.version.kind !== "socket"
    || value.kind !== "socket"
    || !isCapabilities(value.capabilities, "socket")
    || !isProtocolPackageSchema(value.upstream_schema)
    || !isProtocolPackageSchema(value.downstream_schema)) {
    return "内置示例恢复结果不完整，请刷新列表后重试。";
  }
  return undefined;
}

/**
 * Rust 已使用 `semver::Version` 将分组内版本按从旧到新排序。前端只反转这个
 * 权威顺序用于展示，不能再用 JavaScript Number 或字符串实现第二套 SemVer；
 * 否则超大版本号、prerelease 和 build metadata 会与后端选择结果分叉。
 */
export function sortPackageVersions(
  versions: ProtocolPackageVersionViewModel[],
): ProtocolPackageVersionViewModel[] {
  return [...versions].reverse();
}

export function packageStatus(versions: ProtocolPackageVersionViewModel[]) {
  const valid = versions.filter((version) => version.validation.state === "valid");
  const enabledCount = valid.filter((version) => version.enabled).length;
  const invalidCount = versions.length - valid.length;
  if (valid.length === 0) {
    return {
      label: "校验失败",
      color: "danger" as const,
      invalidCount,
      validCount: 0,
    };
  }
  if (enabledCount === valid.length) {
    return {
      label: "已启用",
      color: "success" as const,
      invalidCount,
      validCount: valid.length,
    };
  }
  if (enabledCount > 0) {
    return {
      label: `部分启用 ${enabledCount}/${valid.length}`,
      color: "warning" as const,
      invalidCount,
      validCount: valid.length,
    };
  }
  return {
    label: "已停用",
    color: "default" as const,
    invalidCount,
    validCount: valid.length,
  };
}

export function validationText(validation: ProtocolPackageValidationViewModel) {
  return validation.state === "valid" ? "校验通过" : `校验失败：${validation.code || "未知错误"}`;
}

export function capabilityItems(capabilities: ProtocolPackageCapabilitiesViewModel) {
  return [
    ["上行 Frame", capabilities.upstream.frame],
    ["上行 Decode", capabilities.upstream.decode],
    ["上行 Encode", capabilities.upstream.encode],
    ["下行 Frame", capabilities.downstream.frame],
    ["下行 Decode", capabilities.downstream.decode],
    ["下行 Encode", capabilities.downstream.encode],
    ["Display", capabilities.display],
  ] as const;
}

export function protocolPackageKindText(kind: "http" | "socket"): string {
  return kind === "http" ? "HTTP" : "Socket";
}

/**
 * IPC 类型通常由 Rust 保证，但运行时边界仍可能遇到旧前端缓存、损坏响应或测试
 * 适配器。列表必需字段缺失时整页报错，不能把缺计数伪装成 0 或把缺版本伪装为空包。
 */
export function isProtocolPackageGroupList(
  value: unknown,
): value is ProtocolPackageGroupViewModel[] {
  if (!Array.isArray(value)) return false;
  const groupIds = new Set<string>();
  for (const group of value) {
    if (!isRecord(group)
      || !hasOnly(group, ["id", "name", "kind", "versions", "reference_count", "active_reference_count"])
      || typeof group.id !== "string"
      || group.id.length === 0
      || groupIds.has(group.id)
      || typeof group.name !== "string"
      || group.name.length === 0
      || (group.kind !== "http" && group.kind !== "socket")
      || !Array.isArray(group.versions)
      || group.versions.length === 0
      || !isCounter(group.reference_count)
      || !isCounter(group.active_reference_count)
      || group.active_reference_count > group.reference_count) {
      return false;
    }
    groupIds.add(group.id);
    const versions = new Set<string>();
    for (const version of group.versions) {
      if (!isProtocolPackageVersion(version)
        || version.package.id !== group.id
        || version.kind !== group.kind
        || versions.has(version.package.version)) {
        return false;
      }
      versions.add(version.package.version);
    }
  }
  return true;
}

/**
 * 详情必须完整且属于当前选中的精确身份。任何错配都 fail-closed，避免旧响应或
 * 异常适配器把另一个版本的 Schema 显示在当前版本按钮下面。
 */
export function protocolPackageDetailError(
  value: unknown,
  expected: ProtocolPackageRef,
  expectedKind?: "http" | "socket",
): string | undefined {
  if (!isRecord(value)
    || !hasOnly(value, ["version", "kind", "capabilities", "upstream_schema", "downstream_schema", "usages", "external"])
    || !isProtocolPackageVersion(value.version)) {
    return "协议包详情数据不完整。";
  }
  if (
    value.version.package.id !== expected.id
    || value.version.package.version !== expected.version
  ) {
    return "协议包详情身份与当前选择不一致。";
  }
  if ((value.kind !== "http" && value.kind !== "socket")
    || value.version.kind !== value.kind
    || (expectedKind !== undefined && value.kind !== expectedKind)
    || !isCapabilities(value.capabilities, value.kind)
    || !isProtocolPackageSchema(value.upstream_schema)
    || !isProtocolPackageSchema(value.downstream_schema)
    || !Array.isArray(value.usages)
    || !value.usages.every(isUsage)
    || (value.version.package_source.type === "managed"
      ? value.external !== null
      : !isExternalDetail(value.external))) {
    return "协议包详情数据不完整。";
  }
  return undefined;
}

function isExternalDetail(value: unknown): boolean {
  return isRecord(value)
    && hasOnly(value, [
      "local_process", "remote_address", "connection_id", "first_connected_at", "last_connected_at",
      "registration_fingerprint_sha256", "upstream_methods",
      "downstream_methods", "recent_error",
    ])
    && typeof value.local_process === "boolean"
    && (value.remote_address === null || typeof value.remote_address === "string")
    && (value.connection_id === null || typeof value.connection_id === "string")
    && typeof value.first_connected_at === "string"
    && typeof value.last_connected_at === "string"
    && typeof value.registration_fingerprint_sha256 === "string"
    && /^[0-9a-f]{64}$/.test(value.registration_fingerprint_sha256)
    && isExternalMethods(value.upstream_methods)
    && isExternalMethods(value.downstream_methods)
    && (value.recent_error === null || isExternalRecentError(value.recent_error));
}

function isExternalMethods(value: unknown): boolean {
  return isRecord(value)
    && hasOnly(value, ["frame", "decode", "encode", "display"])
    && [value.frame, value.decode, value.encode, value.display]
      .every((method) => typeof method === "string" && method.length > 0);
}

function isExternalRecentError(value: unknown): boolean {
  return isRecord(value)
    && hasOnly(value, ["code", "message", "occurred_at"])
    && typeof value.code === "string"
    && value.code.length > 0
    && typeof value.message === "string"
    && typeof value.occurred_at === "string";
}

export function isProtocolPackageVersion(
  value: unknown,
): value is ProtocolPackageVersionViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["package", "name", "host_api", "kind", "package_source", "enabled", "validation", "installed_at"])
    || !isRecord(value.package)
    || !hasOnly(value.package, ["id", "version"])) return false;
  const validation = value.validation;
  return typeof value.package.id === "string"
    && value.package.id.length > 0
    && typeof value.package.version === "string"
    && value.package.version.length > 0
    && typeof value.name === "string"
    && value.name.length > 0
    && isCounter(value.host_api)
    && (value.kind === "http" || value.kind === "socket")
    && isProtocolPackageSource(value.package_source)
    && typeof value.enabled === "boolean"
    && typeof value.installed_at === "string"
    && isRecord(validation)
    && ((validation.state === "valid" && hasOnly(validation, ["state"]))
      || (validation.state === "invalid"
        && hasOnly(validation, ["state", "code"])
        && typeof validation.code === "string"));
}

function isCapabilities(value: unknown, kind: "http" | "socket"): boolean {
  if (!isRecord(value) || !hasOnly(value, ["upstream", "downstream", "display"])) return false;
  return isDirectionCapabilities(value.upstream, kind)
    && isDirectionCapabilities(value.downstream, kind)
    && value.display === true;
}

function isDirectionCapabilities(value: unknown, kind: "http" | "socket"): boolean {
  return isRecord(value)
    && hasOnly(value, ["frame", "decode", "encode"])
    && value.frame === (kind === "socket")
    && value.decode === true
    && value.encode === true;
}

function isUsage(value: unknown): boolean {
  return isRecord(value)
    && hasOnly(value, ["workspace_id", "workspace_name", "listener_id", "listener_name", "listener_enabled", "runtime_state"])
    && typeof value.workspace_id === "string"
    && value.workspace_id.length > 0
    && typeof value.workspace_name === "string"
    && value.workspace_name.length > 0
    && typeof value.listener_id === "string"
    && value.listener_id.length > 0
    && typeof value.listener_name === "string"
    && value.listener_name.length > 0
    && typeof value.listener_enabled === "boolean"
    && ["stopped", "starting", "running", "stopping", "faulted"].includes(
      String(value.runtime_state),
    );
}

function isCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function hasOnly(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => actual.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
