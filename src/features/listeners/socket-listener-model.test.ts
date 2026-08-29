import { describe, expect, it } from "vitest";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import {
  appSecurity,
  bindPackage,
  exactPackageKey,
  isListenerProtocolPackageCatalog,
  matchingOption,
  setAppTls,
  setAppTransport,
  setProcessingMode,
  setServerTls,
  setServerTransport,
  setSocketTopology,
  socketCatalogOptions,
} from "./socket-listener-model";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";

function option(
  version = "1.0.0",
  capabilities = {
    upstream: { frame: true, decode: true, encode: true },
    downstream: { frame: true, decode: true, encode: true },
    display: true,
  },
): ListenerProtocolPackageOptionViewModel {
  return {
    package: { id: "iso-8583", version },
    name: `ISO 8583 ${version}`,
    package_source: { type: "internal", built_in: false },
    kind: "socket",
    capabilities,
    upstream_schema: { root: { type: "object", title: "ISO Request", properties: { mti: { type: "string", title: "MTI" } } } },
    downstream_schema: { root: { type: "object", title: "ISO Response", properties: { response_code: { type: "string", title: "Response" } } } },
  };
}

function relaySettings(): SocketRelaySettings {
  return {
    topology: {
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: { mode: "transparent" },
      },
    },
    maximum_connections: 64,
    runtime_limits: defaultSocketRuntimeLimits(),
    processing: {
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "1.0.0" },
      },
    },
  };
}

function localSettings(): SocketRelaySettings {
  return {
    ...relaySettings(),
    topology: {
      mode: "local_responder",
      settings: { downstream_security: { mode: "tcp" } },
    },
  };
}

describe("Socket Listener model", () => {
  it("accepts a complete internally consistent Listener package catalog", () => {
    const candidate = option();
    expect(isListenerProtocolPackageCatalog({
      options: [candidate],
      installed_version_count: 2,
      unavailable_version_count: 1,
      recommended_package: null,
    })).toBe(true);
  });

  it("accepts complete HTTP and Socket package descriptions", () => {
    const candidate = option("1.0.0", {
      upstream: { frame: false, decode: true, encode: true },
      downstream: { frame: false, decode: true, encode: true },
      display: true,
    });
    candidate.kind = "http";
    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
    })).toBe(true);

    candidate.capabilities.downstream.encode = false;
    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
    })).toBe(false);
    candidate.capabilities.downstream.encode = true;
    candidate.capabilities.upstream.frame = true;
    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
    })).toBe(false);
  });

  it("accepts only a recommended exact package that exists in the available options", () => {
    const candidate = option();
    candidate.package = { id: "iso8583-ascii-standard", version: "1.0.0" };
    expect(isListenerProtocolPackageCatalog({
      options: [candidate],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: candidate.package,
    })).toBe(true);
    const wrongKind = { ...candidate, kind: "http" as const };
    expect(isListenerProtocolPackageCatalog({
      options: [wrongKind],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: wrongKind.package,
    })).toBe(false);
    expect(isListenerProtocolPackageCatalog({
      options: [candidate],
      installed_version_count: 1,
      unavailable_version_count: 0,
      recommended_package: { id: candidate.package.id, version: "9.0.0" },
    })).toBe(false);
    const userCandidate = option();
    userCandidate.package = { id: "user-package", version: "1.0.0" };
    expect(isListenerProtocolPackageCatalog({
      options: [candidate, userCandidate],
      installed_version_count: 2,
      unavailable_version_count: 0,
      recommended_package: userCandidate.package,
    })).toBe(false);
  });

  it.each([
    ["non-object", null],
    ["missing field", { options: [], installed_version_count: 0 }],
    ["negative count", { options: [], installed_version_count: -1, unavailable_version_count: -1, recommended_package: null }],
    ["fractional count", { options: [], installed_version_count: 0.5, unavailable_version_count: 0.5, recommended_package: null }],
    ["inconsistent counts", { options: [], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null }],
    ["duplicate exact identity", (() => {
      const candidate = option();
      return { options: [candidate, candidate], installed_version_count: 2, unavailable_version_count: 0, recommended_package: null };
    })()],
    ["invalid package identity", (() => {
      const candidate = option();
      candidate.package.id = "";
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null };
    })()],
    ["invalid Schema", (() => {
      const candidate = option();
      candidate.upstream_schema.root = { type: "array", items: null as never };
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null };
    })()],
    ["invalid capabilities", (() => {
      const candidate = option();
      candidate.capabilities.upstream.decode = "yes" as never;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null };
    })()],
    ["missing required Frame capability", (() => {
      const candidate = option();
      candidate.capabilities.downstream.frame = false;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null };
    })()],
    ["missing required Decode capability", (() => {
      const candidate = option();
      candidate.capabilities.upstream.decode = false;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null };
    })()],
  ])("rejects the entire catalog for %s", (_case, value) => {
    expect(isListenerProtocolPackageCatalog(value)).toBe(false);
  });

  it("binds an exact package as the whole processing configuration", () => {
    expect(bindPackage({ mode: "direct" }, option("2.0.0"), false)).toEqual({
      mode: "scripted",
      settings: { package: { id: "iso-8583", version: "2.0.0" } },
    });
  });

  it("removes Relay-only endpoint and Server security when switching to LocalResponder", () => {
    const settings = relaySettings();
    settings.topology = {
      mode: "relay",
      settings: {
        upstream: { host: "private.example", port: 443 },
        security: {
          mode: "tls_to_tls",
          downstream_tls: { server_identity: "app-cert", client_authentication: { mode: "disabled" } },
          upstream_tls: { verify_hostname: false, tls_server_name: "payments.example.test", server_trust: "server-ca", client_identity: "client-cert" },
        },
      },
    };

    expect(setSocketTopology(settings, "local_responder")).toEqual({
      maximum_connections: 64,
      runtime_limits: defaultSocketRuntimeLimits(),
      topology: {
        mode: "local_responder",
        settings: {
          downstream_security: {
            mode: "tls",
            downstream_tls: { server_identity: "app-cert", client_authentication: { mode: "disabled" } },
          },
        },
      },
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
        },
      },
    });
  });

  it("creates an empty Relay endpoint without inventing Server TLS when leaving LocalResponder", () => {
    const settings = localSettings();
    settings.topology = {
      mode: "local_responder",
      settings: {
        downstream_security: {
          mode: "tls",
          downstream_tls: { server_identity: "app-cert", client_authentication: { mode: "disabled" } },
        },
      },
    };

    expect(setSocketTopology(settings, "relay").topology).toEqual({
      mode: "relay",
      settings: {
        upstream: { host: "", port: 0 },
        security: {
          mode: "tls_to_tcp",
          downstream_tls: { server_identity: "app-cert", client_authentication: { mode: "disabled" } },
        },
      },
    });
  });

  it("emits an exact Direct Relay payload when Direct is selected from LocalResponder", () => {
    expect(setProcessingMode(localSettings(), "direct")).toEqual({
      topology: {
        mode: "relay",
        settings: { upstream: { host: "", port: 0 }, security: { mode: "transparent" } },
      },
      maximum_connections: 64,
      runtime_limits: defaultSocketRuntimeLimits(),
      processing: { mode: "direct" },
    });
  });

  it("emits an exact Scripted payload with no hidden defaults when Scripted is selected", () => {
    const settings = relaySettings();
    settings.processing = { mode: "direct" };

    expect(setProcessingMode(settings, "scripted").processing).toEqual({
      mode: "scripted",
      settings: {
        package: { id: "", version: "" },
      },
    });
  });

  it("matches package versions by exact id and version instead of id alone", () => {
    const catalog: ListenerProtocolPackageCatalogViewModel = {
      options: [option("1.0.0"), option("2.0.0")],
      installed_version_count: 2,
      unavailable_version_count: 0,
      recommended_package: null,
    };

    expect(matchingOption(catalog, { id: "iso-8583", version: "1.0.0" })?.package.version).toBe("1.0.0");
    expect(matchingOption(catalog, { id: "iso-8583", version: "3.0.0" })).toBeUndefined();
    expect(exactPackageKey({ id: "a@b", version: "c" })).not.toBe(exactPackageKey({ id: "a", version: "b@c" }));
  });

  it("keeps HTTP packages out of Socket package selection", () => {
    const socket = option("1.0.0");
    const http = option("2.0.0");
    http.kind = "http";
    const catalog: ListenerProtocolPackageCatalogViewModel = {
      options: [http, socket],
      installed_version_count: 2,
      unavailable_version_count: 0,
      recommended_package: null,
    };

    expect(socketCatalogOptions(catalog)).toEqual([socket]);
    expect(matchingOption(catalog, http.package)).toBeUndefined();
  });

  it("preserves App TLS while changing Server transport independently", () => {
    const appTls = setAppTransport(relaySettings(), "tls");
    const bothTls = setServerTransport(appTls, "tls");

    expect(appSecurity(bothTls)).toEqual({
      mode: "tls",
      downstream_tls: { server_identity: "", client_authentication: { mode: "disabled" } },
    });
    expect(bothTls.topology).toMatchObject({
      settings: { security: { mode: "tls_to_tls", upstream_tls: { verify_hostname: true } } },
    });
  });

  it("updates LocalResponder App TLS without creating Relay fields", () => {
    const tls = { server_identity: "local-id", client_authentication: { mode: "disabled" as const } };

    expect(setAppTls(localSettings(), tls).topology).toEqual({
      mode: "local_responder",
      settings: { downstream_security: { mode: "tls", downstream_tls: tls } },
    });
  });

  it("updates Relay App TLS while preserving Server TLS", () => {
    const current = setServerTransport(relaySettings(), "tls");
    const tls = { server_identity: "app-id", client_authentication: { mode: "disabled" as const } };

    expect(setAppTls(current, tls).topology).toMatchObject({
      settings: {
        security: {
          mode: "tls_to_tls",
          downstream_tls: tls,
          upstream_tls: { verify_hostname: true },
        },
      },
    });
  });

  it("updates Relay Server TLS while preserving App TLS", () => {
    const current = setAppTransport(relaySettings(), "tls");
    const tls = { verify_hostname: false, tls_server_name: "payments.example.test", server_trust: "ca", client_identity: "identity" };

    expect(setServerTls(current, tls).topology).toMatchObject({
      settings: {
        security: {
          mode: "tls_to_tls",
          downstream_tls: { server_identity: "", client_authentication: { mode: "disabled" } },
          upstream_tls: tls,
        },
      },
    });
  });

  it("ignores Server TLS changes for LocalResponder", () => {
    const current = localSettings();

    expect(setServerTls(current, { verify_hostname: true, tls_server_name: null, server_trust: null, client_identity: null })).toBe(current);
  });

  it("ignores invalid or unavailable Server transport selections", () => {
    const local = localSettings();
    const relay = relaySettings();

    expect(setServerTransport(local, "tls")).toBe(local);
    expect(setServerTransport(relay, null as never)).toBe(relay);
  });

  it("preserves existing Server TLS settings when TLS is selected again", () => {
    const current = setServerTls(setServerTransport(relaySettings(), "tls"), {
      verify_hostname: false, tls_server_name: "payments.example.test", server_trust: "ca", client_identity: "identity",
    });

    expect(setServerTransport(current, "tls")).toEqual(current);
  });

  it("returns the same settings when topology or processing mode already matches", () => {
    const relay = relaySettings();

    expect(setSocketTopology(relay, "relay")).toBe(relay);
    expect(setProcessingMode(relay, "scripted")).toBe(relay);
  });

  it("creates Scripted package state when binding from Direct processing", () => {
    expect(bindPackage({ mode: "direct" }, option(), false)).toMatchObject({
      mode: "scripted",
      settings: { package: { id: "iso-8583", version: "1.0.0" } },
    });
  });

  it("returns no package match when the catalog has not loaded", () => {
    expect(matchingOption(undefined, { id: "iso-8583", version: "1.0.0" })).toBeUndefined();
  });

  it.each([
    ["unknown Schema node type", { type: "float" }],
    ["missing array item Schema", { type: "array" }],
    ["empty Schema title", { type: "string", title: "" }],
  ])("rejects a catalog with %s", (_case, root) => {
    const candidate = option();
    candidate.upstream_schema.root = root as never;

    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
    })).toBe(false);
  });

  it("ignores a cleared processing Select instead of creating an unintended Scripted payload", () => {
    const current = relaySettings();
    current.processing = { mode: "direct" };

    expect(setProcessingMode(current, null as never)).toEqual(current);
  });

  it("ignores a cleared topology Select instead of changing topology", () => {
    const current = localSettings();

    expect(setSocketTopology(current, null as never)).toEqual(current);
  });

  it("ignores a cleared transport Select instead of changing App security", () => {
    const current = setAppTransport(localSettings(), "tls");

    expect(setAppTransport(current, null as never)).toEqual(current);
  });

});
