import type { ProxyListener } from "@/generated/rust-types";
import { defaultSocketRuntimeLimits } from "@/features/listeners/listener-data-plane";

export const packageRef = { id: "iso8583", version: "1.2.3" };

export function socketRuleListener(id: string, local = false): ProxyListener {
  return {
    id,
    name: local ? "本地应答" : "交易中继",
    enabled: true,
    bind_address: "127.0.0.1",
    port: local ? 9002 : 9001,
    connect_timeout_ms: 1_000,
    read_timeout_ms: 1_000,
    write_timeout_ms: 1_000,
    data_plane: {
      kind: "socket",
      settings: {
        topology: local
          ? { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } }
          : {
              mode: "relay",
              settings: {
                upstream: { host: "example.test", port: 9000 },
                security: { mode: "transparent" },
              },
            },
        maximum_connections: 8,
        runtime_limits: defaultSocketRuntimeLimits(),
        processing: {
          mode: "scripted",
          settings: { package: packageRef },
        },
      },
    },
  };
}
