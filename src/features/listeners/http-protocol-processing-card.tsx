"use client";

import { Alert, Button, Card, Label, ListBox, Select, Spinner } from "@heroui/react";
import { useState, type Key } from "react";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  HttpListenerSettings,
} from "@/generated/rust-types";
import {
  exactPackageKey,
  httpCatalogOptions,
} from "./socket-listener-model";

interface ProtocolCatalogState {
  data?: ListenerProtocolPackageCatalogViewModel;
  error?: string;
  loading: boolean;
  refresh: () => Promise<void>;
}

const PLAIN_KEY = "__plain_body__";

export function HttpProtocolProcessingCard({ settings, catalog, locked, onChange }: {
  settings: HttpListenerSettings;
  catalog: ProtocolCatalogState;
  locked: boolean;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
}) {
  const [announcement, setAnnouncement] = useState("");
  const body = settings.body_processing;
  const hasBoundPackage = body.mode === "protocol" && body.package.id.length > 0 && body.package.version.length > 0;
  const httpOptions = httpCatalogOptions(catalog.data);
  const selected = catalog.loading || catalog.error
    ? undefined
    : hasBoundPackage
      ? httpOptions.find((item) => exactPackageKey(item.package) === exactPackageKey(body.package))
      : undefined;
  const selectedKey = body.mode === "protocol" && hasBoundPackage
    ? exactPackageKey(body.package)
    : PLAIN_KEY;
  const unavailableBound = body.mode === "protocol"
    && hasBoundPackage
    && !selected
    && !catalog.loading
    && !catalog.error
    && Boolean(catalog.data);
  const optionsCount = httpOptions.length;

  function selectMode(key: Key | null) {
    if (key === null || key === PLAIN_KEY) {
      onChange({ body_processing: { mode: "plain" } });
      setAnnouncement("使用 HTTP 明文 Body 透传（不执行协议包解码/编码）。");
      return;
    }
    const option = httpOptions.find((item) => exactPackageKey(item.package) === key);
    if (!option) return;
    onChange({ body_processing: { mode: "protocol", package: option.package } });
    setAnnouncement(`已选择 ${option.name}；HTTP Body 将按协议包自动解码、规则处理与可逆编码。`);
  }

  function optionText(option: ListenerProtocolPackageOptionViewModel): string {
    const source = option.package_source.online ? "外部 · 在线" : "外部 · 离线";
    return `${option.name} · ${option.package.version} · ${source}`;
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>4. 协议处理</Card.Title>
        <Card.Description>仅支持明文透传或按 HTTP 协议包执行 Body 级解析与回写。</Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4">
        {catalog.loading && <Spinner aria-label="正在读取入口协议包目录" />}
        {catalog.error && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>协议包目录读取失败</Alert.Title>
              <Alert.Description>{catalog.error}</Alert.Description>
              <Button size="sm" variant="outline" onPress={() => void catalog.refresh()}>重试</Button>
            </Alert.Content>
          </Alert>
        )}
        {!catalog.loading && !catalog.error && optionsCount === 0 && (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>没有可绑定的 HTTP 协议包版本</Alert.Title>
              <Alert.Description>
                已安装 {catalog.data?.installed_version_count ?? 0} 个版本，其中 {catalog.data?.unavailable_version_count ?? 0} 个当前不可用。
                HTTP 协议包不会出现在 Socket 入口配置；请先在协议包页面导入、修复或启用兼容的 HTTP 版本。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        {unavailableBound && (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>当前 Body 协议处理已不可用</Alert.Title>
              <Alert.Description>
                精确身份 {body.mode === "protocol" ? `${body.package.id}@${body.package.version}` : ""} 仍会保留，不会自动替换。
                该版本可能已停用、校验失败，或其外部进程已离线。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
          <Select
            aria-label="HTTP 协议处理方案"
            selectedKey={selectedKey}
            isDisabled={locked || catalog.loading || Boolean(catalog.error)}
            onSelectionChange={selectMode}>
            <Label>Body 协议处理</Label>
            <Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              <ListBox.Item id={PLAIN_KEY} textValue="明文透传">
                不使用协议包（明文透传）
              </ListBox.Item>
              {unavailableBound && (
                <ListBox.Item id={selectedKey} isDisabled textValue="当前选择（不可用）">
                  {body.mode === "protocol" ? `${body.package.id}@${body.package.version}` : ""} · 当前选择（不可用）
                </ListBox.Item>
              )}
              {optionsCount > 0 && (!catalog.loading && !catalog.error ? httpOptions : []).map((option) => (
                <ListBox.Item key={`${option.package.id}\u0000${option.package.version}`}
                  id={`${option.package.id}\u0000${option.package.version}`}
                  textValue={optionText(option)}>{optionText(option)}
                </ListBox.Item>
              ))}
            </ListBox></Select.Popover>
          </Select>
        </div>
        {announcement && <p role="status" aria-live="polite" className="text-sm text-[var(--telemetry-muted)]">
          {announcement}
        </p>}
      </Card.Content>
    </Card>
  );
}
