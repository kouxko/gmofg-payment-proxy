import type {
  ProtocolPackageCapabilitiesViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageRef,
  ProtocolPackageValidationViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { BUILT_IN_ISO_8583_PACKAGE } from "@/lib/protocol-package-identity";

export function builtInRestoreResultError(value: unknown): string | undefined {
  if (!isRecord(value)
    || (value.outcome !== "installed" && value.outcome !== "reused")
    || !isProtocolPackageVersion(value.version)
    || value.version.built_in !== true
    || value.version.enabled !== true
    || value.version.validation.state !== "valid"
    || value.version.package.id !== BUILT_IN_ISO_8583_PACKAGE.id
    || value.version.package.version !== BUILT_IN_ISO_8583_PACKAGE.version
    || !isCapabilities(value.capabilities)
    || !isSchema(value.schema)) {
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
      || typeof group.id !== "string"
      || group.id.length === 0
      || groupIds.has(group.id)
      || typeof group.name !== "string"
      || group.name.length === 0
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
): string | undefined {
  if (!isRecord(value) || !isProtocolPackageVersion(value.version)) {
    return "协议包详情数据不完整。";
  }
  if (
    value.version.package.id !== expected.id
    || value.version.package.version !== expected.version
  ) {
    return "协议包详情身份与当前选择不一致。";
  }
  if (!isCapabilities(value.capabilities)
    || !isSchema(value.schema)
    || !Array.isArray(value.usages)
    || !value.usages.every(isUsage)) {
    return "协议包详情数据不完整。";
  }
  return undefined;
}

function isProtocolPackageVersion(
  value: unknown,
): value is ProtocolPackageVersionViewModel {
  if (!isRecord(value) || !isRecord(value.package)) return false;
  const validation = value.validation;
  return typeof value.package.id === "string"
    && value.package.id.length > 0
    && typeof value.package.version === "string"
    && value.package.version.length > 0
    && typeof value.name === "string"
    && value.name.length > 0
    && isCounter(value.host_api)
    && typeof value.built_in === "boolean"
    && typeof value.enabled === "boolean"
    && typeof value.installed_at === "string"
    && isRecord(validation)
    && (validation.state === "valid"
      || (validation.state === "invalid" && typeof validation.code === "string"));
}

function isCapabilities(value: unknown): boolean {
  if (!isRecord(value) || !isRecord(value.upstream) || !isRecord(value.downstream)) {
    return false;
  }
  return [
    value.upstream.frame,
    value.upstream.decode,
    value.upstream.encode,
    value.downstream.frame,
    value.downstream.decode,
    value.downstream.encode,
    value.display,
  ].every((capability) => typeof capability === "boolean");
}

function isSchema(value: unknown): boolean {
  return isRecord(value)
    && typeof value.id === "string"
    && isCounter(value.version)
    && typeof value.title === "string"
    && Array.isArray(value.fields)
    && value.fields.every((field) => isRecord(field)
      && typeof field.name === "string"
      && typeof field.label === "string"
      && ["string", "int", "bool", "blob"].includes(String(field.type)));
}

function isUsage(value: unknown): boolean {
  return isRecord(value)
    && typeof value.workspace_id === "string"
    && typeof value.workspace_name === "string"
    && typeof value.listener_id === "string"
    && typeof value.listener_name === "string"
    && typeof value.listener_enabled === "boolean"
    && ["stopped", "starting", "running", "stopping", "faulted"].includes(
      String(value.runtime_state),
    );
}

function isCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
