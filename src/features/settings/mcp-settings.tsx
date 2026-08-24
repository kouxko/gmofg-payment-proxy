"use client";

import { Alert, Button, Card, Chip, toast } from "@heroui/react";
import { Copy } from "@gravity-ui/icons";
import type { McpInfoViewModel } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

async function copyText(value: string, label: string) {
  try {
    await navigator.clipboard.writeText(value);
    toast(`${label}已复制。`, { variant: "success" });
  } catch (reason) {
    toast(`复制失败：${errorMessage(reason)}`, { variant: "danger" });
  }
}

function configuration(endpoint: string) {
  return JSON.stringify(
    {
      mcpServers: {
        "intercept-proxy": {
          type: "http",
          url: endpoint,
        },
      },
    },
    null,
    2,
  );
}

export function McpSettings() {
  const info = useIpcQuery<McpInfoViewModel>("mcp-info", () =>
    commands.mcpInfo(),
  );

  if (info.error) {
    return (
      <Alert status="danger">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>无法读取 MCP 状态</Alert.Title>
          <Alert.Description>{info.error}</Alert.Description>
        </Alert.Content>
        <Button size="sm" variant="outline" onPress={() => void info.refresh()}>
          重试
        </Button>
      </Alert>
    );
  }

  if (!info.data) {
    return <p className="text-sm text-[var(--telemetry-muted)]">正在读取 MCP 状态…</p>;
  }

  const data = info.data;
  const config = configuration(data.endpoint);
  return (
    <div className="space-y-4">
      <Card className="border border-[var(--telemetry-line)] shadow-none">
        <div className="p-5">
          <div className="flex flex-wrap items-start gap-3">
            <div className="min-w-0 flex-1">
              <h2 className="font-semibold">AI 助手连接（MCP）</h2>
              <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
                让支持 MCP 的 AI 只读查看当前配置、运行状态、抓包、规则和诊断信息，解释问题，并给出代理端与 App 端的修改建议。AI 不能通过此连接修改或启动任何功能。
              </p>
            </div>
            <Chip color={data.available ? "success" : "danger"} variant="soft">
              {data.available ? "服务已就绪" : "服务未启动"}
            </Chip>
          </div>

          <dl className="mt-5 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm max-[760px]:grid-cols-1">
            <dt className="text-[var(--telemetry-muted)]">连接地址</dt>
            <dd className="flex min-w-0 items-center gap-2">
              <code className="min-w-0 break-all rounded bg-[var(--telemetry-soft)] px-2 py-1">{data.endpoint}</code>
              <Button
                isIconOnly
                size="sm"
                variant="outline"
                aria-label="复制 MCP 地址"
                onPress={() => void copyText(data.endpoint, "MCP 地址")}
              >
                <Copy className="size-4" />
              </Button>
            </dd>
            <dt className="text-[var(--telemetry-muted)]">连接方式</dt>
            <dd>{data.transport} · MCP {data.protocol_version}</dd>
            <dt className="text-[var(--telemetry-muted)]">访问范围</dt>
            <dd>{data.access_scope}</dd>
            <dt className="text-[var(--telemetry-muted)]">认证方式</dt>
            <dd>{data.authentication}</dd>
            <dt className="text-[var(--telemetry-muted)]">当前能力</dt>
            <dd>{data.tool_count} 个只读工具 · {data.resource_count} 个参考资源</dd>
          </dl>

          {!data.available && (
            <Alert status="warning" className="mt-4">
              应用通常会自动启动 MCP。当前未启动时，请确认本机 17653 端口没有被其他程序占用，然后重启应用。
            </Alert>
          )}
        </div>
      </Card>

      <Card className="border border-[var(--telemetry-line)] shadow-none">
        <div className="p-5">
          <div className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <h2 className="font-semibold">客户端配置</h2>
              <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
                在支持 Streamable HTTP MCP 的 AI 客户端中添加下面的服务器配置，并保持 Intercept Proxy 正在运行。
              </p>
            </div>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void copyText(config, "MCP 配置")}
            >
              <Copy className="size-4" />复制配置
            </Button>
          </div>
          <pre className="mt-4 overflow-x-auto rounded-xl border border-[var(--telemetry-line)] bg-[var(--telemetry-soft)] p-4 text-xs">
            <code>{config}</code>
          </pre>
        </div>
      </Card>

      <div className="grid grid-cols-2 gap-4 max-[900px]:grid-cols-1">
        <Card className="border border-[var(--telemetry-line)] shadow-none">
          <div className="p-5">
            <h2 className="font-semibold">AI 现在可以帮你做什么</h2>
            <ul className="mt-3 list-disc space-y-2 pl-5 text-sm text-[var(--telemetry-muted)]">
              <li>读取 Workspace、入口、Android 网络接管和当前运行状态。</li>
              <li>分析统一的 HTTP/Socket Exchange 运行记录、断点、规则命中与最近诊断。</li>
              <li>检查协议包、字段结构、四阶段规则和 ISO 8583 示例资料。</li>
              <li>根据证据建议 App 端如何修改代理接入、证书信任、HTTP 或 Socket 客户端。</li>
              <li>给出最小方案、可选方案、风险、回退和验证步骤，不把猜测写成结论。</li>
            </ul>
          </div>
        </Card>
        <Card className="border border-[var(--telemetry-line)] shadow-none">
          <div className="p-5">
            <h2 className="font-semibold">证书概念也能解释</h2>
            <p className="mt-3 text-sm text-[var(--telemetry-muted)]">
              MCP 内置小白指南，能解释 Root CA、服务端证书、客户端证书、私钥、PEM、P12/PFX、应用到代理和代理到上游两段 TLS 的区别，并结合公开证书元数据和诊断判断应先检查哪一段。
            </p>
            <p className="mt-3 text-sm text-[var(--telemetry-muted)]">
              MCP 不会读取或返回私钥、证书密码、PIN 和原始密钥容器，也不会替你导入或删除证书。
            </p>
          </div>
        </Card>
      </div>

      <Alert status="warning">
        MCP 没有认证并只监听本机地址；同一台电脑上的其他进程可能连接并读取上述只读信息。不要在不可信的共享电脑上保持应用运行。
      </Alert>
    </div>
  );
}
