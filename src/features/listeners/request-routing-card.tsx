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
 * 这里仅切换两种已有的 Rust 转发模式：按请求目标转发，或转发到固定 Server。
 * 固定 Server 的 TLS/mTLS 配置仍由同页的“上游 Server 连接安全”卡片负责。
 */
export function RequestRoutingCard({ settings, onChange }: Props) {
  const fixedServer = settings.fixed_server;

  function changeMode(enabled: boolean) {
    onChange({ fixed_server: enabled ? defaultFixedServer() : null });
  }

  return (
    <Card className="col-span-2 max-[700px]:col-span-1">
      <Card.Header>
        <Card.Title>请求转发方式</Card.Title>
        <Card.Description>
          关闭时按客户端请求目标转发；开启后仅替换 host/port，原请求 path 与
          query 保持不变。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-5">
        <div className="flex items-center justify-between gap-4 rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3">
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
        </div>

        {fixedServer && (
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
                  fixed_server: {
                    ...fixedServer,
                    upstream_url: event.target.value,
                  },
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
