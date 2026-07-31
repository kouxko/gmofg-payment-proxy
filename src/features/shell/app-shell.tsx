"use client";

import {
  Alert,
  Button,
  Chip,
  Drawer,
  Modal,
  Separator,
  Spinner,
  Toolbar,
  Tooltip,
} from "@heroui/react";
import {
  Bug,
  Bars,
  CircleInfo,
  DatabaseMagnifier,
  File,
  Gear,
  ListCheck,
  Lock,
  Pulse,
  Shield,
  SlidersVertical,
} from "@gravity-ui/icons";
import { useEffect, useState } from "react";
import { BootstrapProvider, useBootstrap } from "./bootstrap-context";
import { useWorkspaceNavigation } from "./workspace-navigation";
import { toneColor } from "@/lib/format";
import { PageHelp } from "@/features/help/page-help";

export const navigation = [
  {
    href: "/console",
    label: "控制台",
    title: "代理控制台",
    icon: SlidersVertical,
  },
  { href: "/capture", label: "抓包", title: "实时抓包", icon: File },
  {
    href: "/sessions",
    label: "会话",
    title: "会话记录",
    icon: DatabaseMagnifier,
  },
  { href: "/breakpoints", label: "断点", title: "断点实验台", icon: Bug },
  { href: "/rules", label: "规则", title: "拦截规则", icon: ListCheck },
  { href: "/faults", label: "模拟", title: "故障模拟", icon: Pulse },
  {
    href: "/certificates",
    label: "证书",
    title: "证书管理",
    icon: Shield,
  },
  { href: "/settings", label: "设置", title: "系统设置", icon: Gear },
] as const;

export const sideNavigationItemClassName =
  "flex min-h-20 !w-full flex-col items-center justify-center gap-1.5 rounded-xl px-3 text-center text-sm";
export const sideNavigationIconClassName =
  "block size-6 shrink-0 self-center";
export const sideNavigationLabelClassName =
  "block w-14 shrink-0 whitespace-nowrap text-center leading-5";
export const sideNavigationClassName =
  "row-start-2 flex w-24 flex-col gap-2 overflow-y-auto border-r border-[var(--telemetry-line)] bg-white px-2 py-3 max-[1280px]:w-20 max-[1025px]:hidden";
export const shellErrorRegionClassName = "px-5 pt-4";

function CurrentTime() {
  const [currentTime, setCurrentTime] = useState("—");
  useEffect(() => {
    const updateCurrentTime = () =>
      setCurrentTime(new Date().toLocaleString("zh-CN", { hour12: false }));
    updateCurrentTime();
    const timer = window.setInterval(updateCurrentTime, 1000);
    return () => window.clearInterval(timer);
  }, []);
  return <span className="ml-auto tabular-nums">{currentTime}</span>;
}

function GlobalStatusBar() {
  const { proxy, bootstrap, isLoading } = useBootstrap();
  const productName = bootstrap?.product_name ?? "网络代理工具";
  const { pathname, navigate } = useWorkspaceNavigation();
  const [mobileNavigationOpen, setMobileNavigationOpen] = useState(false);

  return (
    <header className="col-span-2 flex min-h-14 items-center overflow-x-auto border-b border-[var(--telemetry-line)] bg-white px-4 max-[1025px]:col-span-1 max-[1025px]:overflow-visible max-[1025px]:py-2">
      <Toolbar className="flex min-w-max flex-1 items-center gap-4 whitespace-nowrap text-sm max-[1025px]:min-w-0 max-[1025px]:flex-wrap max-[1025px]:gap-x-3 max-[1025px]:gap-y-2">
        <Drawer
          isOpen={mobileNavigationOpen}
          onOpenChange={setMobileNavigationOpen}
        >
          <Button
            className="hidden max-[1025px]:inline-flex"
            isIconOnly
            size="sm"
            variant="ghost"
            aria-label="打开主导航"
          >
            <Bars className="size-5" />
          </Button>
          <Drawer.Backdrop>
            <Drawer.Content placement="left">
              <Drawer.Dialog>
                <Drawer.Header>
                  <Drawer.Heading>{productName}</Drawer.Heading>
                </Drawer.Header>
                <Drawer.Body className="space-y-2">
                  {navigation.map(({ href, title, icon: Icon }) => (
                    <Button
                      key={href}
                      variant="ghost"
                      className={[
                        "flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left",
                        pathname === href
                          ? "bg-[var(--telemetry-accent-soft)] text-[var(--telemetry-accent)]"
                          : "hover:bg-[var(--telemetry-soft)]",
                      ].join(" ")}
                      onPress={() => {
                        navigate(href);
                        setMobileNavigationOpen(false);
                      }}
                    >
                      <Icon className="size-5" />
                      {title}
                    </Button>
                  ))}
                </Drawer.Body>
              </Drawer.Dialog>
            </Drawer.Content>
          </Drawer.Backdrop>
        </Drawer>
        <div className="mr-2 flex min-w-56 items-center gap-2 font-semibold">
          <Lock className="size-4 text-[var(--telemetry-accent)]" />
          {productName}
        </div>
        <Separator orientation="vertical" className="h-6" />
        {isLoading && !proxy ? (
          <Spinner size="sm" aria-label="正在加载代理状态" />
        ) : (
          <>
            <Chip
              color={proxy ? toneColor(proxy.ui_tone) : "default"}
              variant="soft"
              size="sm"
            >
              <Chip.Label>{proxy?.state_text ?? "未连接"}</Chip.Label>
            </Chip>
            {proxy?.channels.map((channel) => (
              <div key={channel.id} className="contents">
                <span>
                  {channel.display_name} {channel.state_text} ·{" "}
                  {channel.listen_address}
                </span>
                <Separator orientation="vertical" className="h-5" />
              </div>
            ))}
            <Chip
              color={
                proxy
                  ? toneColor(proxy.proxy_to_server_health.ui_tone)
                  : "default"
              }
              variant="soft"
              size="sm"
            >
              上游 {proxy?.proxy_to_server_health.state_text ?? "状态未知"}
            </Chip>
            <Separator orientation="vertical" className="h-5" />
            <Chip
              color={
                bootstrap?.certificate
                  ? toneColor(bootstrap.certificate.ui_tone)
                  : "default"
              }
              variant="soft"
              size="sm"
            >
              {bootstrap?.certificate.status_text ?? "证书状态未知"}
            </Chip>
            <Separator orientation="vertical" className="h-5" />
            <span>会话数 {proxy?.active_sessions ?? 0}</span>
            <Separator orientation="vertical" className="h-5" />
            <span>暂停数 {proxy?.pending_breakpoints ?? 0}</span>
          </>
        )}
        <CurrentTime />
        <PageHelp pathname={pathname} />
        <Tooltip>
          <Button
            isIconOnly
            size="sm"
            variant="ghost"
            aria-label="打开系统设置"
            onPress={() => {
              navigate("/settings");
            }}
          >
            <Gear className="size-4" />
          </Button>
          <Tooltip.Content>系统设置</Tooltip.Content>
        </Tooltip>
      </Toolbar>
    </header>
  );
}

function SideNavigation() {
  const { bootstrap } = useBootstrap();
  const productName = bootstrap?.product_name ?? "网络代理工具";
  const { pathname, navigate } = useWorkspaceNavigation();
  return (
    <nav
      aria-label="主导航"
      className={sideNavigationClassName}
    >
      {navigation.map(({ href, label, title, icon: Icon }) => {
        const active = pathname === href;
        return (
          <Button
            key={href}
            variant="ghost"
            aria-current={active ? "page" : undefined}
            className={[
              `relative ${sideNavigationItemClassName}`,
              active
                ? "bg-[var(--telemetry-accent-soft)] font-semibold text-[var(--telemetry-accent)]"
                : "text-[var(--telemetry-ink)] hover:bg-[var(--telemetry-soft)]",
            ].join(" ")}
            aria-label={title}
            onPress={() => navigate(href)}
          >
            <Icon className={sideNavigationIconClassName} />
            <span className={sideNavigationLabelClassName}>{label}</span>
          </Button>
        );
      })}
      <div className="mt-auto">
        <Modal>
          <Button
            aria-label="关于"
            variant="ghost"
            className={`${sideNavigationItemClassName} cursor-pointer text-[var(--telemetry-muted)] hover:bg-[var(--telemetry-soft)]`}
          >
            <CircleInfo className={sideNavigationIconClassName} />
            <span className={sideNavigationLabelClassName}>关于</span>
          </Button>
          <Modal.Backdrop>
            <Modal.Container size="sm">
              <Modal.Dialog>
                <Modal.Header>
                  <Modal.Heading>关于 {productName}</Modal.Heading>
                </Modal.Header>
                <Modal.Body className="space-y-3 text-sm">
                  <p>面向客户端联机测试的双向 mTLS 代理与故障注入工具。</p>
                  <p className="text-[var(--telemetry-muted)]">
                    网络、证书、规则、校验、存储和导出均由 Rust
                    核心执行；Next.js 仅负责显示状态和提交用户操作。
                  </p>
                </Modal.Body>
                <Modal.Footer>
                  <Button slot="close" variant="primary">
                    关闭
                  </Button>
                </Modal.Footer>
              </Modal.Dialog>
            </Modal.Container>
          </Modal.Backdrop>
        </Modal>
      </div>
    </nav>
  );
}

function ShellContent({ children }: Readonly<{ children: React.ReactNode }>) {
  const { error, refresh } = useBootstrap();
  return (
    <div className="grid h-full grid-cols-[96px_minmax(0,1fr)] grid-rows-[56px_minmax(0,1fr)] max-[1280px]:grid-cols-[80px_minmax(0,1fr)] max-[1025px]:grid-cols-1 max-[1025px]:grid-rows-[auto_minmax(0,1fr)]">
      <GlobalStatusBar />
      <SideNavigation />
      <main className="min-w-0 overflow-auto bg-white">
        {error && (
          <div className={shellErrorRegionClassName}>
            <Alert status="danger">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>Rust 核心暂不可用</Alert.Title>
                <Alert.Description>{error}</Alert.Description>
              </Alert.Content>
              <Button size="sm" variant="outline" onPress={() => void refresh()}>
                重试
              </Button>
            </Alert>
          </div>
        )}
        {children}
      </main>
    </div>
  );
}

export function AppShell({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <BootstrapProvider>
      <ShellContent>{children}</ShellContent>
    </BootstrapProvider>
  );
}
