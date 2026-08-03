import type { ReactElement } from "react";
import {
  Alert,
  Button,
  Card,
  Input,
  Label,
  ListBox,
  Select,
  Spinner,
} from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidProxyRoute,
  ProxyListener,
} from "@/generated/rust-types";
import { NumericField } from "./android-network-fields";

interface ProxyRoutesCardProps {
  draft: AndroidNetworkProfile;
  listeners: ProxyListener[];
  loading: boolean;
  error?: string;
  onChange: (draft: AndroidNetworkProfile) => void;
}

/** 收集透明路由意图；目标与 Listener 引用的合法性全部由 Rust 校验。 */
export function ProxyRoutesCard({
  draft,
  listeners,
  loading,
  error,
  onChange,
}: ProxyRoutesCardProps): ReactElement {
  const routes = draft.proxy_routes ?? [];

  function updateRoute(index: number, changes: Partial<AndroidProxyRoute>): void {
    onChange({
      ...draft,
      proxy_routes: routes.map((route, current) => (
        current === index ? { ...route, ...changes } : route
      )),
    });
  }

  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>透明代理路由（可多个）</Card.Title>
        <Card.Description>
          业务 App 仍访问原始 Server；设备端 VPN 按原始目标和端口，把匹配连接转交给当前 Workspace 的代理入口。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-3 p-4">
        {error && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>读取代理入口失败</Alert.Title>
              <Alert.Description>{error}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        {loading && <Spinner aria-label="正在读取当前 Workspace 的代理入口" />}
        {!loading && listeners.length === 0 && (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>当前 Workspace 没有代理入口</Alert.Title>
              <Alert.Description>请先到“代理入口配置”新建入口，再返回选择。</Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        {routes.length === 0 && (
          <p className="text-sm text-[var(--telemetry-muted)]">
            当前不转交桌面代理，仅在设备端实施弱网。
          </p>
        )}
        {routes.map((route, routeIndex) => (
          <div
            key={`proxy-route-${routeIndex}`}
            className="space-y-3 rounded-2xl border border-[var(--telemetry-line)] p-3"
          >
            <div className="grid grid-cols-[minmax(0,1fr)_minmax(260px,1fr)_auto] items-end gap-3 max-[820px]:grid-cols-1">
              <div className="grid gap-1">
                <Label>原始目标 {routeIndex + 1}</Label>
                <Input
                  aria-label={`原始目标 ${routeIndex + 1}`}
                  value={route.destination}
                  onChange={(event) => updateRoute(routeIndex, { destination: event.target.value })}
                  placeholder="api.example.test、10.0.34.50 或 CIDR"
                />
              </div>
              <Select
                aria-label={`原始目标 ${routeIndex + 1} 代理入口`}
                selectedKey={route.listener_id || null}
                onSelectionChange={(key) => updateRoute(routeIndex, { listener_id: String(key) })}
              >
                <Label>转交到代理入口</Label>
                <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
                <Select.Popover>
                  <ListBox>
                    {listeners.map((listener) => (
                      <ListBox.Item key={listener.id} id={listener.id} textValue={listener.name}>
                        {listener.name} · {listener.bind_address}:{listener.port}
                      </ListBox.Item>
                    ))}
                  </ListBox>
                </Select.Popover>
              </Select>
              <Button
                variant="danger-soft"
                onPress={() => onChange({
                  ...draft,
                  proxy_routes: routes.filter((_, current) => current !== routeIndex),
                })}
              >
                删除路由
              </Button>
            </div>
            <div className="space-y-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <Label>原始端口</Label>
                <Button
                  size="sm"
                  variant="outline"
                  onPress={() => updateRoute(routeIndex, { ports: [...route.ports, 1] })}
                >
                  添加端口
                </Button>
              </div>
              {route.ports.length === 0 ? (
                <p className="text-xs text-[var(--telemetry-danger)]">
                  必须至少添加一个原始端口；透明代理不会使用“全部端口”匹配。
                </p>
              ) : (
                <div className="flex flex-wrap gap-2">
                  {route.ports.map((port, portIndex) => (
                    <div key={`${routeIndex}-${portIndex}`} className="flex min-w-52 items-end gap-2">
                      <NumericField
                        ariaLabel={`原始目标 ${routeIndex + 1} 端口 ${portIndex + 1}`}
                        label={`端口 ${portIndex + 1}`}
                        minValue={1}
                        maxValue={65_535}
                        value={port}
                        onChange={(value) => updateRoute(routeIndex, {
                          ports: route.ports.map((current, index) => index === portIndex ? value : current),
                        })}
                      />
                      <Button
                        size="sm"
                        variant="danger-soft"
                        onPress={() => updateRoute(routeIndex, {
                          ports: route.ports.filter((_, index) => index !== portIndex),
                        })}
                      >
                        删除端口
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}
        <Button
          variant="outline"
          isDisabled={listeners.length === 0}
          onPress={() => onChange({
            ...draft,
            proxy_routes: [...routes, {
              destination: "",
              ports: [],
              listener_id: listeners[0]?.id ?? "",
            }],
          })}
        >
          添加透明代理路由
        </Button>
        <p className="text-xs text-[var(--telemetry-muted)]">
          设备网络方案不保存桌面 IP、ADB 端口或设备传输地址；这些运行态由 Rust 启动时解析。
        </p>
      </Card.Content>
    </Card>
  );
}
