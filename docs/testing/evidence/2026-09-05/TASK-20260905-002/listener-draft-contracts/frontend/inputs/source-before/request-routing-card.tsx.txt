"use client";

import { Card, Input, Label, ListBox, Select } from "@heroui/react";
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
  const responseMode = localServer
    ? "local_server"
    : fixedServer
      ? "fixed_server"
      : "request_target";

  function changeResponseMode(mode: "request_target" | "fixed_server" | "local_server") {
    onChange({
      topology: mode === "local_server"
        ? { mode: "local_server" }
        : {
            mode: "remote_server",
            settings: {
              fixed_server: mode === "fixed_server" ? defaultFixedServer() : null,
            },
          },
    });
  }

  return (
    <Card className="col-span-2 max-[700px]:col-span-1">
      <Card.Header>
        <Card.Title>1. 响应方式</Card.Title>
        <Card.Description>
          选择把 HTTP 请求转发到上游服务，还是由本机直接生成应答。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-5">
        <div className="grid gap-4 md:grid-cols-2">
          <Select
            aria-label="HTTP 响应方式"
            selectedKey={responseMode}
            onSelectionChange={(key) => {
              if (key === "request_target" || key === "fixed_server" || key === "local_server") {
                changeResponseMode(key);
              }
            }}
          >
            <Label>响应方式</Label>
            <Select.Trigger className="h-10 min-h-10">
              <Select.Value className="truncate" />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover><ListBox>
              <ListBox.Item id="request_target" textValue="按原请求目标转发">按原请求目标转发</ListBox.Item>
              <ListBox.Item id="fixed_server" textValue="转发到固定 Server">转发到固定 Server</ListBox.Item>
              <ListBox.Item id="local_server" textValue="本机应答">本机应答</ListBox.Item>
            </ListBox></Select.Popover>
          </Select>
        </div>

        {!localServer && <div className="rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3">
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
