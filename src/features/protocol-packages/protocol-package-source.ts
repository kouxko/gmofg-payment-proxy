import type {
  ProtocolPackageSourceViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { BUILT_IN_ISO_8583_PACKAGE } from "@/lib/protocol-package-identity";

/**
 * 严格验证 Rust 提供的协议包来源 closed union。
 *
 * IPC 边界不接受已删除的内部 variant、额外字段或缺失的判别字段，也不把离线状态
 * 错误折叠为停用状态。
 */
export function isProtocolPackageSource(
  value: unknown,
): value is ProtocolPackageSourceViewModel {
  if (!isRecord(value)) return false;
  if (value.type === "external") {
    return hasOnly(value, ["type", "online"])
      && typeof value.online === "boolean";
  }
  return false;
}

/** 来源文案只读取 closed union，不从名称、ID 或启用状态反推来源。 */
export function packageSourceText(version: ProtocolPackageVersionViewModel): string {
  return version.package_source.online ? "外部 · 在线" : "外部 · 离线";
}

export function isBuiltInPackage(version: ProtocolPackageVersionViewModel): boolean {
  return version.package.id === BUILT_IN_ISO_8583_PACKAGE.id
    && version.package.version === BUILT_IN_ISO_8583_PACKAGE.version;
}

export function isExternalPackage(version: ProtocolPackageVersionViewModel): boolean {
  return version.package_source.type === "external";
}

function hasOnly(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => actual.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
