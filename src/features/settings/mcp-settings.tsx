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

function configuration(port: number) {
  return JSON.stringify(
    {
      mcpServers: {
        "intercept-proxy": {
          type: "http",
          url: `http://<本机可达地址>:${port}/mcp`,
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
  const config = configuration(data.ipv4.port);
  return (
    <div className="space-y-4">
      <Card className="border border-[var(--telemetry-line)] shadow-none">
        <div className="p-5">
          <div className="flex flex-wrap items-start gap-3">
            <div className="min-w-0 flex-1">
              <h2 className="font-semibold">AI 助手连接（MCP）</h2>
              <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
                让支持 MCP 的 AI 读取诊断与运行信息，并通过候选、技术验证、预览和一次性确认流程原子修改完整 Workspace 配置。MCP 不会自动停止、启动或重启 Listener。
              </p>
            </div>
            <Chip color={data.available ? "success" : "danger"} variant="soft">
              {data.available ? "服务已就绪" : "运行状态异常"}
            </Chip>
          </div>

          <dl className="mt-5 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm max-[760px]:grid-cols-1">
            <dt className="text-[var(--telemetry-muted)]">监听投影</dt>
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
            <dt className="text-[var(--telemetry-muted)]">IPv4</dt>
            <dd>{data.ipv4.available ? `${data.ipv4.bind_address}:${data.ipv4.port} 可用` : "不可用"}</dd>
            <dt className="text-[var(--telemetry-muted)]">IPv6</dt>
            <dd>{data.ipv6.available ? `${data.ipv6.bind_address}:${data.ipv6.port} 可用` : "不可用"}</dd>
            <dt className="text-[var(--telemetry-muted)]">当前能力</dt>
            <dd>{data.tool_count} 个工具（含读写能力） · {data.resource_count} 个参考资源</dd>
          </dl>

          {!data.available && (
            <Alert status="warning" className="mt-4">
              当前运行实例没有返回 MCP 服务状态。IPv4 MCP 绑定失败会直接阻止应用完成启动，因此这不是可忽略的端口冲突降级；请重新读取状态并检查启动日志。
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
                监听投影中的 0.0.0.0 不是客户端地址。把下方占位符替换为运行 Intercept Proxy 这台机器对客户端可达的 IPv4 或 IPv6 地址，并保持应用运行。
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
              <li>分析统一的 HTTP/Socket Exchange 运行记录、规则命中与最近诊断。</li>
              <li>检查协议包、字段结构、四阶段规则和 ISO 8583 示例资料。</li>
              <li>根据证据建议 App 端如何修改代理接入、证书信任、HTTP 或 Socket 客户端。</li>
              <li>提交完整 Workspace 候选，查看技术验证与无私密材料预览，再用一次性确认令牌原子应用。</li>
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
              写入候选可以直接携带证书、私钥和密码；预览、日志、诊断和普通结果不会返回这些私密材料。传输仍是明文 HTTP。
            </p>
          </div>
        </Card>
      </div>

      <Alert status="warning">
        高风险边界：MCP 监听所有可用网络接口，使用明文 HTTP，且没有客户端认证、授权或来源限制。任何能访问该端口的主机都能读取公开数据、提交私钥和密码，并修改 Proxy 配置；网络窃听者也可能读取传输内容和确认令牌。
      </Alert>
      {data.warning_codes.length > 0 && (
        <Alert status="warning">
          网络能力告警：{data.warning_codes.join("、")}。IPv4 仍可用时服务会继续运行，但界面不会把不可用的 IPv6 声称为可达。
        </Alert>
      )}
    </div>
  );
}
