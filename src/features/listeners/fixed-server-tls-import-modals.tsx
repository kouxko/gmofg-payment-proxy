"use client";

import { Button, Input, Label, Modal, TextField } from "@heroui/react";

type CommonProps = {
  open: boolean;
  busy: boolean;
  label: string;
  onOpenChange: (open: boolean) => void;
  onLabelChange: (value: string) => void;
};

export function ImportIdentityModal({
  open,
  busy,
  label,
  password,
  onOpenChange,
  onLabelChange,
  onPasswordChange,
  onImport,
}: CommonProps & {
  password: string;
  onPasswordChange: (value: string) => void;
  onImport: () => Promise<void>;
}) {
  return (
    <Modal isOpen={open} onOpenChange={onOpenChange}>
      <Modal.Backdrop isDismissable={!busy}>
        <Modal.Container size="sm">
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>导入 Proxy → Server 的客户端身份</Modal.Heading>
            </Modal.Header>
            <Modal.Body className="space-y-3">
              <TextField>
                <Label>显示名称</Label>
                <Input
                  value={label}
                  onChange={(event) => onLabelChange(event.target.value)}
                />
              </TextField>
              <TextField>
                <Label>client.p12 / client.pfx 密码（允许为空）</Label>
                <Input
                  type="password"
                  value={password}
                  onChange={(event) => onPasswordChange(event.target.value)}
                />
              </TextField>
              <p className="text-sm text-[var(--telemetry-muted)]">
                选择包含“客户端证书 + 私钥”的 client.p12 或 client.pfx。代理连接上游 Server 时出示它；它不是本入口给 Android/App 使用的服务端证书。
              </p>
              <p className="text-xs text-[var(--telemetry-muted)]">
                文件由 Rust 原生对话框读取和解析，导入后会在当前页面显示主题、SAN、有效期和 SHA-256；私钥和密码不会进入 Workspace。
              </p>
            </Modal.Body>
            <Modal.Footer>
              <Button slot="close" variant="outline" isDisabled={busy}>取消</Button>
              <Button
                variant="primary"
                isDisabled={busy || !label.trim()}
                onPress={() => void onImport()}
              >
                选择 client.p12 / .pfx
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

export function ImportTrustModal({
  open,
  busy,
  label,
  onOpenChange,
  onLabelChange,
  onImport,
}: CommonProps & { onImport: () => Promise<void> }) {
  return (
    <Modal isOpen={open} onOpenChange={onOpenChange}>
      <Modal.Backdrop isDismissable={!busy}>
        <Modal.Container size="sm">
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>导入用于验证上游 Server 的 CA</Modal.Heading>
            </Modal.Header>
            <Modal.Body className="space-y-3">
              <TextField>
                <Label>显示名称</Label>
                <Input
                  value={label}
                  onChange={(event) => onLabelChange(event.target.value)}
                />
              </TextField>
              <p className="text-sm text-[var(--telemetry-muted)]">
                选择签发上游 Server 证书的 ca.crt、chain.crt 或 PEM 证书链。不要选择带私钥的 Server 身份文件，也不要用客户端 client.p12 代替。
              </p>
              <p className="text-xs text-[var(--telemetry-muted)]">
                Rust 会立即解析证书，导入后在当前页面显示主题、SAN、有效期和 SHA-256，并在“测试上游 TLS / mTLS 握手”中实际验证。
              </p>
            </Modal.Body>
            <Modal.Footer>
              <Button slot="close" variant="outline" isDisabled={busy}>取消</Button>
              <Button
                variant="primary"
                isDisabled={busy || !label.trim()}
                onPress={() => void onImport()}
              >
                选择 CA 证书（.crt / .pem）
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
