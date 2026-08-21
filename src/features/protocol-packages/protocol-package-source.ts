import type {
  ProtocolPackageSourceViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";

/**
 * 严格验证 Rust 提供的协议包来源 closed union。
 *
 * IPC 边界不接受未知 variant、额外字段或缺失的判别字段，避免旧前端缓存把外部包
 * 误当作内部脚本执行，也避免把离线状态错误折叠为停用状态。
 */
export function isProtocolPackageSource(
  value: unknown,
): value is ProtocolPackageSourceViewModel {
  if (!isRecord(value)) return false;
  if (value.type === "internal") {
    return hasOnly(value, ["type", "built_in"])
      && typeof value.built_in === "boolean";
  }
  if (value.type === "external") {
    return hasOnly(value, ["type", "online"])
      && typeof value.online === "boolean";
  }
  return false;
}

/** 来源文案只读取 closed union，不从名称、ID 或启用状态反推来源。 */
export function packageSourceText(version: ProtocolPackageVersionViewModel): string {
  if (version.package_source.type === "internal") {
    return version.package_source.built_in ? "内置示例" : "用户安装";
  }
  return version.package_source.online ? "外部 · 在线" : "外部 · 离线";
}

export function isBuiltInPackage(version: ProtocolPackageVersionViewModel): boolean {
  return version.package_source.type === "internal" && version.package_source.built_in;
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
