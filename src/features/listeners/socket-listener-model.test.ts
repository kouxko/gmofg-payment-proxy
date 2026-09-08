import { describe, expect, it } from "vitest";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  SocketRelaySecurityDraft,
  SocketRelayDraftSettings,
} from "@/generated/rust-types";
import { BUILT_IN_ISO_8583_PACKAGE } from "@/lib/protocol-package-identity";
import {
  appSecurity,
  bindPackage,
  exactPackageKey,
  httpCatalogOptions,
  isListenerProtocolPackageCatalog,
  matchingOption,
  socketCatalogOptions,
  updateAppTls,
  updateServerTls,
} from "./socket-listener-model";
import { socketListener } from "./listeners-view.test-support";

function option(kind: "http" | "socket" = "socket"): ListenerProtocolPackageOptionViewModel {
  return {
    package: { id: "iso-8583", version: "1.0.0" },
    name: "ISO 8583",
    package_source: { type: "external", online: true },
    kind,
    capabilities: {
      upstream: { frame: kind === "socket", decode: true, encode: true },
      downstream: { frame: kind === "socket", decode: true, encode: true },
      display: true,
    },
    upstream_schema: kind === "socket" ? { root: { type: "object", title: "Request", properties: {} } } : null,
    downstream_schema: kind === "socket" ? { root: { type: "object", title: "Response", properties: {} } } : null,
  };
}

function settings(): SocketRelayDraftSettings {
  const listener = socketListener();
  if (listener.data_plane.kind !== "socket") throw new Error("expected Socket");
  return listener.data_plane.settings;
}

function relayWithSecurity(security: SocketRelaySecurityDraft): SocketRelayDraftSettings {
  const current = settings();
  if (current.topology.mode !== "relay") throw new Error("expected Relay");
  return {
    ...current,
    topology: {
      mode: "relay",
      settings: { ...current.topology.settings, security },
    },
  };
}

describe("Socket Listener presentation model", () => {
  it("validates and filters the authoritative package catalog", () => {
    const socket = option();
    const http = { ...option("http"), package: { id: "http-package", version: "1.0.0" } };
    const catalog: ListenerProtocolPackageCatalogViewModel = {
      options: [socket, http],
      installed_version_count: 2,
      unavailable_version_count: 0,
      recommended_package: null,
    };

    expect(isListenerProtocolPackageCatalog(catalog)).toBe(true);
    expect(socketCatalogOptions(catalog)).toEqual([socket]);
    expect(matchingOption(catalog, socket.package)).toEqual(socket);
    expect(matchingOption(catalog, null)).toBeUndefined();
    expect(exactPackageKey(socket.package)).toBe("iso-8583\u00001.0.0");
  });

  it("rejects malformed or duplicate package identities", () => {
    const candidate = option();
    expect(isListenerProtocolPackageCatalog({
      options: [candidate, candidate],
      installed_version_count: 2,
      unavailable_version_count: 0,
      recommended_package: null,
    })).toBe(false);
    expect(isListenerProtocolPackageCatalog({
      options: [{ ...candidate, package: { id: "", version: "1.0.0" } }],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: null,
    })).toBe(false);
  });

  it("fails closed for malformed catalog counts, recommendations and package contracts", () => {
    const candidate = option();
    const catalog = (overrides: Record<string, unknown>) => ({
      options: [candidate],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: null,
      ...overrides,
    });

    expect(isListenerProtocolPackageCatalog(catalog({ installed_version_count: -1 }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({ unavailable_version_count: 0.5 }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({ recommended_package: { id: "", version: "1.0.0" } }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({ recommended_package: { id: "other", version: "1.0.0" } }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({ options: [{ ...candidate, name: "" }] }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({
      options: [{ ...candidate, capabilities: { ...candidate.capabilities, display: false } }],
    }))).toBe(false);
    expect(isListenerProtocolPackageCatalog(catalog({
      options: [{ ...candidate, upstream_schema: null }],
    }))).toBe(false);
  });

  it("accepts only the installed built-in Socket package as the recommendation", () => {
    const recommended = { ...option(), package: BUILT_IN_ISO_8583_PACKAGE };
    const catalog: ListenerProtocolPackageCatalogViewModel = {
      options: [recommended],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: BUILT_IN_ISO_8583_PACKAGE,
    };

    expect(isListenerProtocolPackageCatalog(catalog)).toBe(true);
    expect(isListenerProtocolPackageCatalog({
      ...catalog,
      options: [{ ...option("http"), package: BUILT_IN_ISO_8583_PACKAGE }],
    })).toBe(false);
  });

  it("returns empty package choices when the authoritative catalog is unavailable", () => {
    expect(socketCatalogOptions(undefined)).toEqual([]);
    expect(httpCatalogOptions(undefined)).toEqual([]);
    expect(matchingOption(undefined, option().package)).toBeUndefined();
  });

  it("binds an explicitly selected exact package without constructing mode defaults", () => {
    const selected = option();
    expect(bindPackage({ mode: "scripted", settings: { package: null } }, selected)).toEqual({
      mode: "scripted",
      settings: { package: selected.package },
    });
  });

  it("updates only fields inside already-enabled App and Server TLS branches", () => {
    const current = settings();
    const both: SocketRelayDraftSettings = {
      ...current,
      topology: {
        mode: "relay",
        settings: {
          ...current.topology.mode === "relay" ? current.topology.settings : {
            upstream: { host: "", port: 0 },
            security: { mode: "transparent" as const },
          },
          security: {
            mode: "tls_to_tls",
            downstream_tls: {
              server_identity: null,
              client_authentication: { mode: "disabled" },
            },
            upstream_tls: {
              verify_hostname: true,
              tls_server_name: null,
              server_trust: null,
              client_identity: null,
            },
          },
        },
      },
    };
    const appTls = { server_identity: "app-id", client_authentication: { mode: "optional" as const, trust: null } };
    const serverTls = { verify_hostname: false, tls_server_name: "server.test", server_trust: null, client_identity: null };

    const appUpdated = updateAppTls(both, appTls);
    const serverUpdated = updateServerTls(appUpdated, serverTls);

    expect(appSecurity(serverUpdated)).toEqual({ mode: "tls", downstream_tls: appTls });
    expect(serverUpdated.topology.mode).toBe("relay");
    if (serverUpdated.topology.mode === "relay") {
      expect(serverUpdated.topology.settings.security).toMatchObject({
        mode: "tls_to_tls",
        upstream_tls: serverTls,
      });
    }
  });

  it("updates each one-sided TLS mode without changing its transport mode", () => {
    const appTls = { server_identity: "app-id", client_authentication: { mode: "disabled" as const } };
    const serverTls = { verify_hostname: false, tls_server_name: "server.test", server_trust: null, client_identity: null };
    const appUpdated = updateAppTls(relayWithSecurity({
      mode: "tls_to_tcp",
      downstream_tls: { server_identity: null, client_authentication: { mode: "disabled" } },
    }), appTls);
    const serverUpdated = updateServerTls(relayWithSecurity({
      mode: "tcp_to_tls",
      upstream_tls: { verify_hostname: true, tls_server_name: null, server_trust: null, client_identity: null },
    }), serverTls);

    expect(appUpdated.topology).toMatchObject({
      mode: "relay",
      settings: { security: { mode: "tls_to_tcp", downstream_tls: appTls } },
    });
    expect(serverUpdated.topology).toMatchObject({
      mode: "relay",
      settings: { security: { mode: "tcp_to_tls", upstream_tls: serverTls } },
    });
  });

  it("does not enable TLS while updating a transport mode that has no matching TLS side", () => {
    const transparent = relayWithSecurity({ mode: "transparent" });
    const appTls = { server_identity: "app-id", client_authentication: { mode: "disabled" as const } };
    const serverTls = { verify_hostname: false, tls_server_name: null, server_trust: null, client_identity: null };

    expect(updateAppTls(transparent, appTls)).toEqual(transparent);
    expect(updateServerTls(transparent, serverTls)).toEqual(transparent);
    expect(appSecurity(transparent)).toEqual({ mode: "tcp" });
  });
});
