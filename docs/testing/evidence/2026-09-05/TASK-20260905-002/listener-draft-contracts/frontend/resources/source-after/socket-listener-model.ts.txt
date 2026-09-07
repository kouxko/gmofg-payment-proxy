import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  ProtocolPackageRef,
  SocketDownstreamSecurityDraft,
  SocketDownstreamTlsDraftSettings,
  SocketPayloadProcessingDraft,
  SocketRelayDraftSettings,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";
import { BUILT_IN_ISO_8583_PACKAGE } from "@/lib/protocol-package-identity";
import { isProtocolPackageSchema } from "@/lib/protocol-package-schema";
import { isProtocolPackageSource } from "@/features/protocol-packages/protocol-package-source";
import { socketDownstreamTls } from "./listener-data-plane";

export type SocketWorkingMode = "raw_relay" | "protocol_relay" | "local_response";

export function socketWorkingMode(settings: SocketRelayDraftSettings): SocketWorkingMode {
  if (settings.topology.mode === "local_responder") return "local_response";
  return settings.processing.mode === "scripted" ? "protocol_relay" : "raw_relay";
}

export function exactPackageKey(value: ProtocolPackageRef): string {
  return `${value.id}\u0000${value.version}`;
}

export function appSecurity(settings: SocketRelayDraftSettings): SocketDownstreamSecurityDraft {
  if (settings.topology.mode === "local_responder") {
    return settings.topology.settings.downstream_security;
  }
  const tls = socketDownstreamTls(settings.topology.settings.security);
  return tls ? { mode: "tls", downstream_tls: tls } : { mode: "tcp" };
}

export function updateAppTls(
  settings: SocketRelayDraftSettings,
  tls: SocketDownstreamTlsDraftSettings,
): SocketRelayDraftSettings {
  if (settings.topology.mode === "local_responder") {
    return {
      ...settings,
      topology: {
        mode: "local_responder",
        settings: { downstream_security: { mode: "tls", downstream_tls: tls } },
      },
    };
  }
  const relay = settings.topology.settings;
  const security = relay.security.mode === "tls_to_tls"
    ? { ...relay.security, downstream_tls: tls }
    : relay.security.mode === "tls_to_tcp"
      ? { ...relay.security, downstream_tls: tls }
      : relay.security;
  return {
    ...settings,
    topology: { mode: "relay", settings: { ...relay, security } },
  };
}

export function updateServerTls(
  settings: SocketRelayDraftSettings,
  tls: SocketUpstreamTlsSettings,
): SocketRelayDraftSettings {
  if (settings.topology.mode !== "relay") return settings;
  const relay = settings.topology.settings;
  const security = relay.security.mode === "tls_to_tls"
    ? { ...relay.security, upstream_tls: tls }
    : relay.security.mode === "tcp_to_tls"
      ? { ...relay.security, upstream_tls: tls }
      : relay.security;
  return {
    ...settings,
    topology: { mode: "relay", settings: { ...relay, security } },
  };
}

export function bindPackage(
  processing: SocketPayloadProcessingDraft,
  option: ListenerProtocolPackageOptionViewModel,
): SocketPayloadProcessingDraft {
  void processing;
  return { mode: "scripted", settings: { package: option.package } };
}

export function matchingOption(
  catalog: ListenerProtocolPackageCatalogViewModel | undefined,
  packageRef: ProtocolPackageRef | null,
): ListenerProtocolPackageOptionViewModel | undefined {
  if (!packageRef) return undefined;
  const key = exactPackageKey(packageRef);
  return socketCatalogOptions(catalog).find((item) => exactPackageKey(item.package) === key);
}

export function socketCatalogOptions(
  catalog: ListenerProtocolPackageCatalogViewModel | undefined,
): ListenerProtocolPackageOptionViewModel[] {
  return catalog?.options.filter((item) => item.kind === "socket") ?? [];
}

export function httpCatalogOptions(
  catalog: ListenerProtocolPackageCatalogViewModel | undefined,
): ListenerProtocolPackageOptionViewModel[] {
  return catalog?.options.filter((item) => item.kind === "http") ?? [];
}

/**
 * IPC 即使有生成类型，运行时仍可能来自旧 Host、损坏缓存或错误测试适配器。
 * Listener 选择器必须整批拒绝畸形目录，不能把缺失能力误当成 false 后继续保存。
 */
export function isListenerProtocolPackageCatalog(
  value: unknown,
): value is ListenerProtocolPackageCatalogViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["options", "installed_version_count", "unavailable_version_count", "recommended_package"])
    || !Array.isArray(value.options)
    || !isCount(value.installed_version_count)
    || !isCount(value.unavailable_version_count)
    || (value.recommended_package !== null && !isPackageRef(value.recommended_package))
    || value.options.length + value.unavailable_version_count !== value.installed_version_count) {
    return false;
  }
  const identities = new Set<string>();
  for (const option of value.options) {
    if (!isCatalogOption(option)) return false;
    const identity = exactPackageKey(option.package);
    if (identities.has(identity)) return false;
    identities.add(identity);
  }
  const recommended = value.recommended_package as ProtocolPackageRef | null;
  if (recommended !== null
    && (exactPackageKey(recommended) !== exactPackageKey(BUILT_IN_ISO_8583_PACKAGE)
      || !value.options.some((option) => option.kind === "socket"
        && exactPackageKey(option.package) === exactPackageKey(recommended)))) return false;
  return true;
}

function isCatalogOption(value: unknown): value is ListenerProtocolPackageOptionViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["package", "name", "package_source", "kind", "capabilities", "upstream_schema", "downstream_schema"])
    || !isPackageRef(value.package)
    || typeof value.name !== "string"
    || value.name.length === 0
    || !isProtocolPackageSource(value.package_source)
    || (value.kind !== "http" && value.kind !== "socket")
    || !isCapabilities(value.capabilities, value.kind)
    || !areSchemasValid(value.kind, value.upstream_schema, value.downstream_schema)) {
    return false;
  }
  return true;
}

function areSchemasValid(
  kind: "http" | "socket",
  upstream: unknown,
  downstream: unknown,
): boolean {
  return isSchemaValidForKind(kind, upstream) && isSchemaValidForKind(kind, downstream);
}

function isSchemaValidForKind(kind: "http" | "socket", schema: unknown): boolean {
  return schema === null ? kind === "http" : isProtocolPackageSchema(schema);
}

function isPackageRef(value: unknown): value is ProtocolPackageRef {
  return isRecord(value)
    && hasOnly(value, ["id", "version"])
    && typeof value.id === "string"
    && value.id.length > 0
    && typeof value.version === "string"
    && value.version.length > 0;
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
    // 当前 Manifest 只允许 Socket 双向声明 Frame、HTTP 双向不声明 Frame。
    && value.frame === (kind === "socket")
    && value.decode === true
    && value.encode === true;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function hasOnly(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => actual.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
