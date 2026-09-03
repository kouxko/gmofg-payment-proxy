"use client";

import { Card, Input, Label, Switch } from "@heroui/react";
import type {
  FixedServerSettings,
  HttpListenerSettings,
} from "@/generated/rust-types";

type Props = {
  settings: HttpListenerSettings;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
};

/**
 * 当前监听的请求去向。
 *
 * 这里切换进程内 LocalServer 或真实 Server；真实 Server 再区分请求目标与固定目标。
 * 固定 Server 的 TLS/mTLS 配置仍由同页的“上游 Server 连接安全”卡片负责。
 */
export function RequestRoutingCard({ settings, onChange }: Props) {
  const localServer = settings.topology.mode === "local_server";
  const fixedServer = settings.topology.mode === "remote_server"
    ? settings.topology.settings.fixed_server
    : null;

  function changeMode(enabled: boolean) {
    onChange({
      topology: {
        mode: "remote_server",
        settings: { fixed_server: enabled ? defaultFixedServer() : null },
      },
    });
  }

  function changeLocalServer(enabled: boolean) {
    onChange({
      topology: enabled
        ? { mode: "local_server" }
        : { mode: "remote_server", settings: { fixed_server: null } },
    });
  }

  return (
    <Card className="col-span-2 max-[700px]:col-span-1">
      <Card.Header>
        <Card.Title>HTTP Server 模式</Card.Title>
        <Card.Description>
          真实 Server 会转发请求；LocalServer 在进程内原样回环，并继续执行上下行规则。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-5">
        <div className="flex items-center justify-between gap-4 rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3">
          <div>
            <p className="font-medium">{localServer ? "Local HTTP Server" : "真实 Server"}</p>
            <p className="text-sm text-[var(--telemetry-muted)]">
              {localServer
                ? "不连接外部 Server；Proxy→Server 与 Proxy→App 仍经过同一套规则。"
                : "按请求目标或固定 Server 建立真实上游连接。"}
            </p>
          </div>
          <Switch aria-label="使用 Local HTTP Server" isSelected={localServer} onChange={changeLocalServer}>
            <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content>
          </Switch>
        </div>

        {!localServer && <div className="flex items-center justify-between gap-4 rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3">
          <div>
            <p className="font-medium">
              {fixedServer ? "转发到固定 Server" : "按原请求目标转发"}
            </p>
            <p className="text-sm text-[var(--telemetry-muted)]">
              {fixedServer
                ? "仅用 Server URL 替换目标 host/port；原请求 path 与 query 原样保留。"
                : "读取每个请求中的目标主机和端口，适用于标准 HTTP/HTTPS 正向代理。"}
            </p>
          </div>
          <Switch
            aria-label="转发到固定 Server"
            isSelected={fixedServer !== null}
            onChange={changeMode}
          >
            <Switch.Content>
              <Switch.Control><Switch.Thumb /></Switch.Control>
            </Switch.Content>
          </Switch>
        </div>}

        {!localServer && fixedServer && (
          <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-4">
            <div>
              <h3 className="font-semibold">固定 Server 目标</h3>
              <p className="text-sm text-[var(--telemetry-muted)]">
                为当前监听指定唯一 host/port；不同监听可以使用不同的地址、端口和证书。
              </p>
            </div>
            <div className="grid gap-1">
              <Label>Server URL</Label>
              <Input
                aria-label="固定 Server URL"
                value={fixedServer.upstream_url}
                onChange={(event) => onChange({
                  topology: { mode: "remote_server", settings: { fixed_server: {
                      ...fixedServer,
                      upstream_url: event.target.value,
                    } } },
                })}
                placeholder="https://api.example.test:443"
              />
            </div>
          </section>
        )}
      </Card.Content>
    </Card>
  );
}

function defaultFixedServer(): FixedServerSettings {
  return {
    upstream_url: "",
    upstream_tls: {
      verify_hostname: true,
      server_trust: null,
      client_identity: null,
    },
  };
}
