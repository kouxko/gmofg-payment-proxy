import { describe, expect, it } from "vitest";
import type {
  ScriptedSocketProcessing,
  SocketDownstreamTlsSettings,
  SocketRelaySettings,
} from "@/generated/rust-types";
import {
  setSocketWorkingMode,
  socketWorkingMode,
  type SocketWorkingMode,
} from "./socket-listener-model";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";

const appTls: SocketDownstreamTlsSettings = {
  server_identity: "app-identity",
  client_authentication: { mode: "required", trust: "app-ca" },
};

const fullProcessing: ScriptedSocketProcessing = {
  package: { id: "iso-8583", version: "2.0.0" },
};

const localProcessing: ScriptedSocketProcessing = {
  package: { id: "iso-8583", version: "2.0.0" },
};

const emptyProcessing: ScriptedSocketProcessing = {
  package: { id: "", version: "" },
};

function relay(processing: SocketRelaySettings["processing"]): SocketRelaySettings {
  return {
    topology: {
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: {
          mode: "tls_to_tls",
          downstream_tls: appTls,
          upstream_tls: {
            verify_hostname: false,
            tls_server_name: "payments.example.test",
            server_trust: "server-ca",
            client_identity: "client-identity",
          },
        },
      },
    },
    maximum_connections: 64,
    runtime_limits: defaultSocketRuntimeLimits(),
    processing,
  };
}

function local(processing: SocketRelaySettings["processing"]): SocketRelaySettings {
  return {
    topology: {
      mode: "local_responder",
      settings: { downstream_security: { mode: "tls", downstream_tls: appTls } },
    },
    maximum_connections: 64,
    runtime_limits: defaultSocketRuntimeLimits(),
    processing,
  };
}

const fixtures: Record<SocketWorkingMode, SocketRelaySettings> = {
  raw_relay: relay({ mode: "direct" }),
  protocol_relay: relay({ mode: "scripted", settings: fullProcessing }),
  local_response: local({ mode: "scripted", settings: localProcessing }),
};

const emptyRelay = {
  mode: "relay" as const,
  settings: {
    upstream: { host: "", port: 0 },
    security: { mode: "tls_to_tcp" as const, downstream_tls: appTls },
  },
};

describe("Socket user working mode", () => {
  it.each([
    ["raw_relay", "relay", "direct"],
    ["protocol_relay", "relay", "scripted"],
    ["local_response", "local_responder", "scripted"],
  ] as const)("maps %s to the exact existing wire", (mode, topology, processing) => {
    const result = setSocketWorkingMode(fixtures.raw_relay, mode);

    expect(result.topology.mode).toBe(topology);
    expect(result.processing.mode).toBe(processing);
    expect(socketWorkingMode(result)).toBe(mode);
  });

  it.each([
    ["raw_relay", "raw_relay", fixtures.raw_relay],
    ["raw_relay", "protocol_relay", {
      ...fixtures.raw_relay,
      processing: { mode: "scripted", settings: emptyProcessing },
    }],
    ["raw_relay", "local_response", local({ mode: "scripted", settings: emptyProcessing })],
    ["protocol_relay", "raw_relay", {
      ...fixtures.protocol_relay,
      processing: { mode: "direct" },
    }],
    ["protocol_relay", "protocol_relay", fixtures.protocol_relay],
    ["protocol_relay", "local_response", local({ mode: "scripted", settings: localProcessing })],
    ["local_response", "raw_relay", {
      ...fixtures.local_response,
      topology: emptyRelay,
      processing: { mode: "direct" },
    }],
    ["local_response", "protocol_relay", {
      ...fixtures.local_response,
      topology: emptyRelay,
    }],
    ["local_response", "local_response", fixtures.local_response],
  ] as const)("switches %s -> %s atomically", (source, target, expected) => {
    const result = setSocketWorkingMode(fixtures[source], target);

    expect(result).toEqual(expected);
    expect(socketWorkingMode(result)).toBe(target);
    if (source === target) expect(result).toBe(fixtures[source]);
  });

  it("ignores an unknown work mode instead of creating a partial wire", () => {
    expect(setSocketWorkingMode(fixtures.protocol_relay, "future_mode" as SocketWorkingMode))
      .toBe(fixtures.protocol_relay);
  });

  it.each(["protocol_relay", "local_response"] as const)(
    "binds the recommended exact package when first entering %s",
    (mode) => {
      const recommended = { id: "iso8583-ascii-standard", version: "1.0.0" };

      const result = setSocketWorkingMode(fixtures.raw_relay, mode, recommended);

      expect(result.processing).toMatchObject({
        mode: "scripted",
        settings: { package: recommended },
      });
    },
  );

  it("does not replace an existing exact binding with the recommendation", () => {
    const result = setSocketWorkingMode(
      fixtures.protocol_relay,
      "local_response",
      { id: "iso8583-ascii-standard", version: "1.0.0" },
    );

    expect(result.processing).toMatchObject({
      mode: "scripted",
      settings: { package: { id: "iso-8583", version: "2.0.0" } },
    });
  });
});
