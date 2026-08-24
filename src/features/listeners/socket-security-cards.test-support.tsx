import { render } from "@testing-library/react";
import { vi } from "vitest";
import type {
  CertificateReference,
  ListenerUpstreamConnectionTestViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { SocketAppSecurityCard, SocketServerCard } from "./socket-security-cards";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";

export const references: CertificateReference[] = [
  { id: "app-id", label: "App Identity", kind: "reverse_server_identity", reference: "managed:app-id" },
  { id: "app-ca", label: "App CA", kind: "downstream_client_trust", reference: "managed:app-ca" },
  { id: "server-ca", label: "Server CA", kind: "upstream_server_trust", reference: "managed:server-ca" },
  { id: "client-id", label: "Client Identity", kind: "upstream_client_identity", reference: "managed:client-id" },
];

export function relay(security: "transparent" | "tls_to_tls" = "tls_to_tls"): SocketRelaySettings {
  return {
    topology: {
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: security === "transparent" ? { mode: "transparent" } : {
          mode: "tls_to_tls",
          downstream_tls: {
            server_identity: "app-id",
            client_authentication: { mode: "required", trust: "app-ca" },
          },
          upstream_tls: {
            verify_hostname: true,
            tls_server_name: null,
            server_trust: "server-ca",
            client_identity: "client-id",
          },
        },
      },
    },
    maximum_connections: 32,
    runtime_limits: defaultSocketRuntimeLimits(),
    processing: { mode: "direct" },
  };
}

export function local(): SocketRelaySettings {
  return {
    ...relay(),
    topology: { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } },
  };
}

export function common(settings: SocketRelaySettings, overrides: { locked?: boolean; busy?: boolean } = {}) {
  return {
    settings,
    certificateReferences: references,
    certificateDetails: references.map((reference) => ({
      reference_id: reference.id, label: reference.label, certificate: null, error_message: null,
    })),
    locked: overrides.locked ?? false,
    busy: overrides.busy ?? false,
    onChange: vi.fn(),
  };
}

export function renderApp(settings = relay(), overrides: { locked?: boolean; busy?: boolean } = {}) {
  const props = {
    ...common(settings, overrides),
    onImportIdentity: vi.fn().mockResolvedValue(true),
    onImportTrust: vi.fn().mockResolvedValue(true),
  };
  render(<SocketAppSecurityCard {...props} />);
  return props;
}

export function renderServer(settings = relay(), overrides: {
  locked?: boolean;
  busy?: boolean;
  testing?: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
} = {}) {
  const props = {
    ...common(settings, overrides),
    testing: overrides.testing ?? false,
    testResult: overrides.testResult,
    testError: overrides.testError,
    onImportIdentity: vi.fn().mockResolvedValue(true),
    onImportTrust: vi.fn().mockResolvedValue(true),
    onTest: vi.fn().mockResolvedValue(undefined),
  };
  render(<SocketServerCard {...props} />);
  return props;
}

export function testResult(tls = true): ListenerUpstreamConnectionTestViewModel {
  return {
    listener_id: "listener", data_plane: "socket", upstream_origin: "server.test:9443",
    resolved_address: "192.0.2.1:9443", scheme: tls ? "tls" : "tcp", transport: tls ? "TLS" : "TCP",
    tls: tls ? {
      tls_version: "TLSv1.3", cipher_suite: "TLS_AES_256_GCM_SHA384", peer_subject: "CN=server.test",
      peer_sha256_fingerprint: "AA:BB", hostname_verification_enabled: true, client_identity_configured: true,
    } : null,
    socket_transport_mode: tls ? "tls_to_tls" : "transparent", tls_server_name_candidates: [], elapsed_millis: 12, message: "连接成功", ui_tone: "positive",
  };
}
