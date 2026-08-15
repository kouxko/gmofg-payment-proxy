import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  ProtocolPackageRef,
  ScriptedSocketProcessing,
  SocketDownstreamSecurity,
  SocketDownstreamTlsSettings,
  SocketPayloadProcessing,
  SocketRelaySecurity,
  SocketRelaySettings,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";
import { socketDownstreamTls, socketUpstreamTls } from "./listener-data-plane";

export function exactPackageKey(value: ProtocolPackageRef): string {
  return `${value.id}\u0000${value.version}`;
}

export function defaultDownstreamTls(): SocketDownstreamTlsSettings {
  return { server_identity: "", client_authentication: { mode: "disabled" } };
}

function emptyScripted(packageRef?: ProtocolPackageRef): ScriptedSocketProcessing {
  return {
    package: packageRef ?? { id: "", version: "" },
    upstream: { decode_enabled: false, encode_enabled: false },
    downstream: { decode_enabled: false, encode_enabled: false },
  };
}

export function appSecurity(settings: SocketRelaySettings): SocketDownstreamSecurity {
  if (settings.topology.mode === "local_responder") {
    return settings.topology.settings.downstream_security;
  }
  const tls = socketDownstreamTls(settings.topology.settings.security);
  return tls ? { mode: "tls", downstream_tls: tls } : { mode: "tcp" };
}

function relaySecurity(
  appTls: SocketDownstreamTlsSettings | undefined,
  upstreamTls: SocketUpstreamTlsSettings | undefined,
): SocketRelaySecurity {
  if (appTls && upstreamTls) return { mode: "tls_to_tls", downstream_tls: appTls, upstream_tls: upstreamTls };
  if (appTls) return { mode: "tls_to_tcp", downstream_tls: appTls };
  if (upstreamTls) return { mode: "tcp_to_tls", upstream_tls: upstreamTls };
  return { mode: "transparent" };
}

export function setAppTransport(settings: SocketRelaySettings, mode: "tcp" | "tls"): SocketRelaySettings {
  if (mode !== "tcp" && mode !== "tls") return settings;
  const existing = appSecurity(settings);
  const appTls = mode === "tls"
    ? existing.mode === "tls" ? existing.downstream_tls : defaultDownstreamTls()
    : undefined;
  if (settings.topology.mode === "local_responder") {
    return {
      ...settings,
      topology: { mode: "local_responder", settings: {
        downstream_security: appTls ? { mode: "tls", downstream_tls: appTls } : { mode: "tcp" },
      } },
    };
  }
  const relay = settings.topology.settings;
  return { ...settings, topology: { mode: "relay", settings: {
    ...relay,
    security: relaySecurity(appTls, socketUpstreamTls(relay.security)),
  } } };
}

export function setAppTls(settings: SocketRelaySettings, tls: SocketDownstreamTlsSettings): SocketRelaySettings {
  if (settings.topology.mode === "local_responder") {
    return { ...settings, topology: { mode: "local_responder", settings: {
      downstream_security: { mode: "tls", downstream_tls: tls },
    } } };
  }
  const relay = settings.topology.settings;
  return { ...settings, topology: { mode: "relay", settings: {
    ...relay,
    security: relaySecurity(tls, socketUpstreamTls(relay.security)),
  } } };
}

export function setServerTransport(settings: SocketRelaySettings, mode: "tcp" | "tls"): SocketRelaySettings {
  if (mode !== "tcp" && mode !== "tls") return settings;
  if (settings.topology.mode !== "relay") return settings;
  const relay = settings.topology.settings;
  const upstreamTls = mode === "tls"
    ? socketUpstreamTls(relay.security) ?? { verify_hostname: true, server_trust: null, client_identity: null }
    : undefined;
  const downstream = socketDownstreamTls(relay.security);
  return { ...settings, topology: { mode: "relay", settings: {
    ...relay,
    security: relaySecurity(downstream, upstreamTls),
  } } };
}

export function setServerTls(settings: SocketRelaySettings, tls: SocketUpstreamTlsSettings): SocketRelaySettings {
  if (settings.topology.mode !== "relay") return settings;
  const relay = settings.topology.settings;
  return { ...settings, topology: { mode: "relay", settings: {
    ...relay,
    security: relaySecurity(socketDownstreamTls(relay.security), tls),
  } } };
}

export function setSocketTopology(settings: SocketRelaySettings, mode: "relay" | "local_responder"): SocketRelaySettings {
  if (mode !== "relay" && mode !== "local_responder") return settings;
  if (settings.topology.mode === mode) return settings;
  if (mode === "local_responder") {
    const security = appSecurity(settings);
    const current = settings.processing?.mode === "scripted"
      ? settings.processing.settings
      : emptyScripted();
    return {
      ...settings,
      topology: { mode: "local_responder", settings: { downstream_security: security } },
      processing: { mode: "scripted", settings: {
        ...current,
        upstream: { ...current.upstream, encode_enabled: false },
        downstream: { ...current.downstream, decode_enabled: false },
      } },
    };
  }
  const security = appSecurity(settings);
  return {
    ...settings,
    topology: { mode: "relay", settings: {
      upstream: { host: "", port: 0 },
      security: relaySecurity(security.mode === "tls" ? security.downstream_tls : undefined, undefined),
    } },
  };
}

export function setProcessingMode(settings: SocketRelaySettings, mode: "direct" | "scripted"): SocketRelaySettings {
  if (mode !== "direct" && mode !== "scripted") return settings;
  if (mode === "direct") {
    const relaySettings = settings.topology.mode === "relay"
      ? settings
      : setSocketTopology(settings, "relay");
    return { ...relaySettings, processing: { mode: "direct" } };
  }
  if (settings.processing?.mode === "scripted") return settings;
  return { ...settings, processing: { mode: "scripted", settings: emptyScripted() } };
}

export function bindPackage(
  processing: SocketPayloadProcessing | undefined,
  option: ListenerProtocolPackageOptionViewModel,
  local: boolean,
): SocketPayloadProcessing {
  const current = processing?.mode === "scripted" ? processing.settings : emptyScripted();
  return { mode: "scripted", settings: {
    ...current,
    package: option.package,
    upstream: {
      ...current.upstream,
      decode_enabled: current.upstream.decode_enabled && option.capabilities.upstream.decode,
      encode_enabled: !local && current.upstream.encode_enabled && option.capabilities.upstream.encode,
    },
    downstream: {
      ...current.downstream,
      decode_enabled: !local && current.downstream.decode_enabled && option.capabilities.downstream.decode,
      encode_enabled: current.downstream.encode_enabled && option.capabilities.downstream.encode,
    },
  } };
}

export function matchingOption(
  catalog: ListenerProtocolPackageCatalogViewModel | undefined,
  packageRef: ProtocolPackageRef,
): ListenerProtocolPackageOptionViewModel | undefined {
  const key = exactPackageKey(packageRef);
  return catalog?.options.find((item) => exactPackageKey(item.package) === key);
}

/**
 * IPC 即使有生成类型，运行时仍可能来自旧 Host、损坏缓存或错误测试适配器。
 * Listener 选择器必须整批拒绝畸形目录，不能把缺失能力误当成 false 后继续保存。
 */
export function isListenerProtocolPackageCatalog(
  value: unknown,
): value is ListenerProtocolPackageCatalogViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["options", "installed_version_count", "unavailable_version_count"])
    || !Array.isArray(value.options)
    || !isCount(value.installed_version_count)
    || !isCount(value.unavailable_version_count)
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
  return true;
}

function isCatalogOption(value: unknown): value is ListenerProtocolPackageOptionViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["package", "name", "capabilities", "schema"])
    || !isPackageRef(value.package)
    || typeof value.name !== "string"
    || value.name.length === 0
    || !isCapabilities(value.capabilities)
    || !isRecord(value.schema)
    || !hasOnly(value.schema, ["id", "version", "title", "fields"])
    || typeof value.schema.id !== "string"
    || value.schema.id.length === 0
    || typeof value.schema.version !== "number"
    || !Number.isSafeInteger(value.schema.version)
    || value.schema.version < 1
    || typeof value.schema.title !== "string"
    || !Array.isArray(value.schema.fields)
    || value.schema.fields.length === 0) {
    return false;
  }
  const names = new Set<string>();
  for (const field of value.schema.fields) {
    if (!isRecord(field)
      || !hasOnly(field, ["name", "type", "label"])
      || typeof field.name !== "string"
      || field.name.length === 0
      || names.has(field.name)
      || typeof field.type !== "string"
      || !["string", "int", "bool", "blob"].includes(field.type)
      || typeof field.label !== "string") return false;
    names.add(field.name);
  }
  return true;
}

function isPackageRef(value: unknown): value is ProtocolPackageRef {
  return isRecord(value)
    && hasOnly(value, ["id", "version"])
    && typeof value.id === "string"
    && value.id.length > 0
    && typeof value.version === "string"
    && value.version.length > 0;
}

function isCapabilities(value: unknown): boolean {
  if (!isRecord(value) || !hasOnly(value, ["upstream", "downstream", "display"])) return false;
  return isDirectionCapabilities(value.upstream)
    && isDirectionCapabilities(value.downstream)
    && typeof value.display === "boolean";
}

function isDirectionCapabilities(value: unknown): boolean {
  return isRecord(value)
    && hasOnly(value, ["frame", "decode", "encode"])
    // 当前 Host API 的 Frame/Decode 是必需入口；false 表示伪造或旧响应，整批拒绝。
    && value.frame === true
    && value.decode === true
    && typeof value.encode === "boolean";
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
