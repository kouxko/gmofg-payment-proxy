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
} from "./socket-listener-model";

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
    capabilities,
    schema: { id: "iso-message", version: 1, title: "ISO Message", fields: [] },
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
    processing: {
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "1.0.0" },
        upstream: { decode_enabled: false, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: false },
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
    candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];

    expect(isListenerProtocolPackageCatalog({
      options: [candidate],
      installed_version_count: 2,
      unavailable_version_count: 1,
    })).toBe(true);
  });

  it("accepts optional Encode and Display capabilities declared false", () => {
    const candidate = option("1.0.0", {
      upstream: { frame: true, decode: true, encode: false },
      downstream: { frame: true, decode: true, encode: false },
      display: false,
    });
    candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];

    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0,
    })).toBe(true);
  });

  it.each([
    ["non-object", null],
    ["missing field", { options: [], installed_version_count: 0 }],
    ["negative count", { options: [], installed_version_count: -1, unavailable_version_count: -1 }],
    ["fractional count", { options: [], installed_version_count: 0.5, unavailable_version_count: 0.5 }],
    ["inconsistent counts", { options: [], installed_version_count: 1, unavailable_version_count: 0 }],
    ["duplicate exact identity", (() => {
      const candidate = option();
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      return { options: [candidate, candidate], installed_version_count: 2, unavailable_version_count: 0 };
    })()],
    ["invalid package identity", (() => {
      const candidate = option();
      candidate.package.id = "";
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0 };
    })()],
    ["invalid Schema", (() => {
      const candidate = option();
      candidate.schema.version = 0;
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0 };
    })()],
    ["invalid capabilities", (() => {
      const candidate = option();
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      candidate.capabilities.upstream.decode = "yes" as never;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0 };
    })()],
    ["missing required Frame capability", (() => {
      const candidate = option();
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      candidate.capabilities.downstream.frame = false;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0 };
    })()],
    ["missing required Decode capability", (() => {
      const candidate = option();
      candidate.schema.fields = [{ name: "mti", label: "MTI", type: "string" }];
      candidate.capabilities.upstream.decode = false;
      return { options: [candidate], installed_version_count: 1, unavailable_version_count: 0 };
    })()],
  ])("rejects the entire catalog for %s", (_case, value) => {
    expect(isListenerProtocolPackageCatalog(value)).toBe(false);
  });

  it("binds a full-capability package without changing any of the sixteen Relay combinations", () => {
    for (let mask = 0; mask < 16; mask += 1) {
      const settings = relaySettings();
      const flags = [0, 1, 2, 3].map((bit) => Boolean(mask & (1 << bit)));
      settings.processing = {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: flags[0], encode_enabled: flags[1] },
          downstream: { decode_enabled: flags[2], encode_enabled: flags[3] },
        },
      };

      expect(bindPackage(settings.processing, option(), false)).toEqual({
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: flags[0], encode_enabled: flags[1] },
          downstream: { decode_enabled: flags[2], encode_enabled: flags[3] },
        },
      });
    }
  });

  it("binds all four LocalResponder combinations while forcing impossible directions off", () => {
    for (let mask = 0; mask < 4; mask += 1) {
      const settings = localSettings();
      settings.processing = {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          // 故意注入两个非法 true，证明生产边界会关闭它们，而不是测试字面量自比较。
          upstream: { decode_enabled: Boolean(mask & 1), encode_enabled: true },
          downstream: { decode_enabled: true, encode_enabled: Boolean(mask & 2) },
        },
      };

      expect(bindPackage(settings.processing, option(), true)).toEqual({
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: Boolean(mask & 1), encode_enabled: false },
          downstream: { decode_enabled: false, encode_enabled: Boolean(mask & 2) },
        },
      });
    }
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
          upstream_tls: { verify_hostname: false, server_trust: "server-ca", client_identity: "client-cert" },
        },
      },
    };

    expect(setSocketTopology(settings, "local_responder")).toEqual({
      maximum_connections: 64,
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
          upstream: { decode_enabled: false, encode_enabled: false },
          downstream: { decode_enabled: false, encode_enabled: false },
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
        upstream: { decode_enabled: false, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: false },
      },
    });
  });

  it("turns off capabilities unsupported by a newly bound exact package version", () => {
    const settings = relaySettings();
    if (settings.processing?.mode !== "scripted") throw new Error("expected scripted settings");
    settings.processing.settings.upstream = { decode_enabled: true, encode_enabled: true };
    settings.processing.settings.downstream = { decode_enabled: true, encode_enabled: true };

    expect(bindPackage(settings.processing, option("2.0.0", {
      upstream: { frame: true, decode: false, encode: true },
      downstream: { frame: true, decode: true, encode: false },
      display: false,
    }), false)).toEqual({
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "2.0.0" },
        upstream: { decode_enabled: false, encode_enabled: true },
        downstream: { decode_enabled: true, encode_enabled: false },
      },
    });
  });

  it("keeps LocalResponder forced directions off even when a package supports them", () => {
    const settings = localSettings();
    if (settings.processing?.mode !== "scripted") throw new Error("expected scripted settings");
    settings.processing.settings.upstream.encode_enabled = true;
    settings.processing.settings.downstream.decode_enabled = true;

    expect(bindPackage(settings.processing, option("2.0.0"), true)).toMatchObject({
      settings: {
        upstream: { encode_enabled: false },
        downstream: { decode_enabled: false },
      },
    });
  });

  it("matches package versions by exact id and version instead of id alone", () => {
    const catalog: ListenerProtocolPackageCatalogViewModel = {
      options: [option("1.0.0"), option("2.0.0")],
      installed_version_count: 2,
      unavailable_version_count: 0,
    };

    expect(matchingOption(catalog, { id: "iso-8583", version: "1.0.0" })?.package.version).toBe("1.0.0");
    expect(matchingOption(catalog, { id: "iso-8583", version: "3.0.0" })).toBeUndefined();
    expect(exactPackageKey({ id: "a@b", version: "c" })).not.toBe(exactPackageKey({ id: "a", version: "b@c" }));
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
    const tls = { verify_hostname: false, server_trust: "ca", client_identity: "identity" };

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

    expect(setServerTls(current, { verify_hostname: true, server_trust: null, client_identity: null })).toBe(current);
  });

  it("ignores invalid or unavailable Server transport selections", () => {
    const local = localSettings();
    const relay = relaySettings();

    expect(setServerTransport(local, "tls")).toBe(local);
    expect(setServerTransport(relay, null as never)).toBe(relay);
  });

  it("preserves existing Server TLS settings when TLS is selected again", () => {
    const current = setServerTls(setServerTransport(relaySettings(), "tls"), {
      verify_hostname: false, server_trust: "ca", client_identity: "identity",
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
    ["duplicate Schema field", [{ name: "mti", label: "MTI", type: "string" }, { name: "mti", label: "Again", type: "int" }]],
    ["unknown Schema field type", [{ name: "mti", label: "MTI", type: "float" }]],
    ["missing Schema field label", [{ name: "mti", type: "string" }]],
  ])("rejects a catalog with %s", (_case, fields) => {
    const candidate = option();
    candidate.schema.fields = fields as never;

    expect(isListenerProtocolPackageCatalog({
      options: [candidate], installed_version_count: 1, unavailable_version_count: 0,
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

  it("migrates a legacy missing processing field to an explicit strict Scripted payload", () => {
    const legacy = { ...relaySettings(), processing: undefined } as unknown as SocketRelaySettings;

    expect(setProcessingMode(legacy, "scripted").processing).toEqual({
      mode: "scripted",
      settings: {
        package: { id: "", version: "" },
        upstream: { decode_enabled: false, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: false },
      },
    });
  });

  it("migrates a legacy missing processing field to LocalResponder with forced directions disabled", () => {
    const legacy = { ...relaySettings(), processing: undefined } as unknown as SocketRelaySettings;

    expect(setSocketTopology(legacy, "local_responder").processing).toEqual({
      mode: "scripted",
      settings: {
        package: { id: "", version: "" },
        upstream: { decode_enabled: false, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: false },
      },
    });
  });
});
