import type {
  AppErrorViewModel,
  ProtocolPackageImportPreviewViewModel,
  ProtocolPackageImportViewModel,
  ProtocolPackageRef,
} from "@/generated/rust-types";
import { isProtocolPackageSchema } from "@/lib/protocol-package-schema";

export type ImportPreviewDisplay = Omit<ProtocolPackageImportPreviewViewModel, "token">;
export type CommittableImportPreview = ProtocolPackageImportPreviewViewModel & {
  token: string;
  disposition: "new" | "reusable";
};

/**
 * 单一判别状态避免 `preview + error + busy` 形成不可能组合。尤其 commit 发出后，
 * 状态中立刻移除一次性 token；后续任何错误都不能重新出现“确认安装”。
 */
export type ProtocolPackageImportState =
  | { kind: "closed" }
  | { kind: "preparing" }
  | { kind: "ready"; preview: CommittableImportPreview }
  | { kind: "conflict"; preview: ImportPreviewDisplay }
  | { kind: "prepare-error"; error: ImportErrorPresentation }
  | { kind: "committing"; preview: ImportPreviewDisplay }
  | { kind: "commit-error"; error: ImportErrorPresentation }
  | { kind: "refreshing"; packageRef: ProtocolPackageRef; outcome: ProtocolPackageImportViewModel["outcome"] }
  | { kind: "refresh-error"; packageRef: ProtocolPackageRef; outcome: ProtocolPackageImportViewModel["outcome"]; error: ImportErrorPresentation }
  | { kind: "discarding"; preview: ImportPreviewDisplay }
  | { kind: "discard-error"; preview: CommittableImportPreview; error: ImportErrorPresentation };

export function withoutImportToken(
  preview: ProtocolPackageImportPreviewViewModel,
): ImportPreviewDisplay {
  const { token: _consumedToken, ...display } = preview;
  void _consumedToken;
  return display;
}

/**
 * 导入响应来自 IPC 边界。即使生成类型在编译期完整，旧 WebView 或损坏适配器仍可能
 * 返回缺字段对象；确认按钮必须 fail-closed，不能把不完整预览的 token 交给 commit。
 */
export function isImportPreview(
  value: unknown,
): value is ProtocolPackageImportPreviewViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["token", "disposition", "package", "name", "host_api", "kind", "capabilities", "upstream_schema", "downstream_schema"])
    || !isRecord(value.package)) return false;
  const disposition = value.disposition;
  const tokenMatchesDisposition = disposition === "identity_conflict"
    ? value.token === null
    : (disposition === "new" || disposition === "reusable")
      && typeof value.token === "string"
      && value.token.length > 0;
  return tokenMatchesDisposition
    && typeof value.package.id === "string"
    && value.package.id.length > 0
    && typeof value.package.version === "string"
    && value.package.version.length > 0
    && typeof value.name === "string"
    && value.name.length > 0
    && isCounter(value.host_api)
    && (value.kind === "http" || value.kind === "socket")
    && isCapabilities(value.capabilities, value.kind)
    && isProtocolPackageSchema(value.upstream_schema)
    && isProtocolPackageSchema(value.downstream_schema);
}

export function isCommittableImportPreview(
  preview: ProtocolPackageImportPreviewViewModel,
): preview is CommittableImportPreview {
  return preview.token !== null
    && (preview.disposition === "new" || preview.disposition === "reusable");
}

/** commit 必须确认后端返回的仍是 prepare 时冻结的同一精确身份。 */
export function importResultError(
  value: unknown,
  preview: ProtocolPackageImportPreviewViewModel,
): string | undefined {
  if (!isRecord(value)
    || !hasOnly(value, ["outcome", "version", "kind", "capabilities", "upstream_schema", "downstream_schema"])
    || (value.outcome !== "installed" && value.outcome !== "reused")
    || !isRecord(value.version)
    || !isRecord(value.version.package)
    || value.version.package.id !== preview.package.id
    || value.version.package.version !== preview.package.version
    || value.kind !== preview.kind
    || !isCapabilities(value.capabilities, preview.kind)
    || !isProtocolPackageSchema(value.upstream_schema)
    || !isProtocolPackageSchema(value.downstream_schema)) {
    return "协议包导入结果与已确认预览不一致，请刷新列表后重试。";
  }
  return undefined;
}

export interface ImportErrorPresentation {
  code?: string;
  message: string;
  details: string[];
}

/**
 * Rust 会把 ZIP、TOML、Schema、Rhai 与入口错误的位置写入 message/field_errors。
 * 前端只按原文展示，不根据错误码重建解析器，也不猜测文件、行列。
 */
export function presentImportError(reason: unknown): ImportErrorPresentation {
  const appError = asAppError(reason);
  if (!appError) {
    return {
      message: "无法连接应用核心，请确认桌面应用已完成初始化。",
      details: [],
    };
  }
  return {
    code: appError.code || undefined,
    message: appError.message,
    details: Array.from(new Set([
      ...Object.values(appError.field_errors).flat(),
      ...diagnosticDetails(appError.diagnostic),
    ])).filter((detail) => detail.trim().length > 0),
  };
}

function diagnosticDetails(diagnostic: AppErrorViewModel["diagnostic"]): string[] {
  if (!diagnostic) return [];
  const location = [diagnostic.file, diagnostic.line, diagnostic.column]
    .filter((part) => part !== null && part !== "")
    .join(":");
  return [
    location,
    diagnostic.field ? `字段：${diagnostic.field}` : "",
    diagnostic.entry ? `入口：${diagnostic.entry}` : "",
  ].filter(Boolean);
}

export function outcomeText(outcome: ProtocolPackageImportViewModel["outcome"]): string {
  return outcome === "reused" ? "相同协议包已存在，已复用精确版本。" : "协议包安装成功。";
}

function asAppError(value: unknown): AppErrorViewModel | undefined {
  if (!isRecord(value)
    || typeof value.code !== "string"
    || typeof value.message !== "string"
    || !isRecord(value.field_errors)) return undefined;
  const fieldErrors = Object.values(value.field_errors);
  if (!fieldErrors.every((messages) => Array.isArray(messages)
    && messages.every((message) => typeof message === "string"))) return undefined;
  return value as AppErrorViewModel;
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
