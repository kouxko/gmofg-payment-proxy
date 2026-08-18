"use client";

import { useEffect, useRef, useState } from "react";
import { Button, Input, Label, Modal, TextField } from "@heroui/react";

type CommonProps = {
  open: boolean;
  busy: boolean;
  label: string;
  onOpenChange: (open: boolean) => void;
  onLabelChange: (value: string) => void;
};

type PemModalProps = CommonProps & {
  title: string;
  description: string;
  detail: string;
  buttonLabel: string;
  onImport: () => Promise<void>;
};

export function ImportPemModal({
  open,
  busy,
  label,
  title,
  description,
  detail,
  buttonLabel,
  onOpenChange,
  onLabelChange,
  onImport,
}: PemModalProps) {
  return (
    <Modal isOpen={open} onOpenChange={onOpenChange}>
      <Button className="hidden" aria-hidden="true">
        打开证书导入对话框
      </Button>
      <Modal.Backdrop isDismissable={!busy}>
        <Modal.Container size="sm" scroll="inside">
          <Modal.Dialog>
            <Modal.Header><Modal.Heading>{title}</Modal.Heading></Modal.Header>
            <Modal.Body className="min-h-0 space-y-4 pr-1">
              <TextField>
                <Label>显示名称</Label>
                <Input
                  value={label}
                  onChange={(event) => onLabelChange(event.target.value)}
                />
              </TextField>
              <p className="text-sm text-[var(--telemetry-muted)]">{description}</p>
              <p className="text-xs text-[var(--telemetry-muted)]">{detail}</p>
            </Modal.Body>
            <Modal.Footer>
              <Button slot="close" variant="outline" isDisabled={busy}>取消</Button>
              <Button
                variant="primary"
                isDisabled={busy || !label.trim()}
                onPress={() => void onImport()}
              >
                {buttonLabel}
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

export function ImportIdentityModal({
  open,
  busy,
  label,
  password,
  onOpenChange,
  onLabelChange,
  onPasswordChange,
  onImport,
  title = "导入 Proxy → Server 的客户端身份",
  description,
  detail,
  buttonLabel = "选择身份文件",
  buttonAriaLabel = "选择客户端身份（.p12 / .pfx / .pem）",
  passwordLabel = "P12 / PFX 密码（PEM 不使用；允许为空）",
}: CommonProps & {
  password: string;
  onPasswordChange: (value: string) => void;
  onImport: () => Promise<void>;
  title?: string;
  description?: string;
  detail?: string;
  buttonLabel?: string;
  buttonAriaLabel?: string;
  passwordLabel?: string;
}) {
  const submittingRef = useRef(false);
  const mountedRef = useRef(true);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => () => {
    mountedRef.current = false;
  }, []);

  async function submit() {
    if (submittingRef.current || busy || !label.trim()) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await onImport();
    } finally {
      submittingRef.current = false;
      if (mountedRef.current) setSubmitting(false);
    }
  }

  return (
    <Modal isOpen={open} onOpenChange={onOpenChange}>
      <Button className="hidden" aria-hidden="true">
        打开客户端身份导入对话框
      </Button>
      <Modal.Backdrop isDismissable={!busy}>
        <Modal.Container size="sm">
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>{title}</Modal.Heading>
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
                <Label>{passwordLabel}</Label>
                <Input
                  type="password"
                  value={password}
                  onChange={(event) => onPasswordChange(event.target.value)}
                />
              </TextField>
              <div className="space-y-2 rounded-2xl border border-[var(--telemetry-line)] p-3">
                <p className="text-sm text-[var(--telemetry-muted)]">
                  {description ?? <>支持 client.p12 / client.pfx，或同时包含客户端证书链与匹配私钥的
                    client.pem。代理连接上游 Server 时出示它；它不是本入口给
                    Android/App 使用的服务端证书。</>}
                </p>
                <p className="text-xs text-[var(--telemetry-muted)]">
                  {detail ?? <>文件通过系统对话框读取和解析，导入后会显示主题、SAN、
                    有效期和 SHA-256。私钥由系统保护存储，输入的密码仅用于本次解密，
                    不会保存在 Workspace 或诊断信息中。</>}
                </p>
              </div>
            </Modal.Body>
            <Modal.Footer className="shrink-0 flex-wrap border-t border-[var(--telemetry-line)] pt-4">
              <Button slot="close" variant="outline" isDisabled={busy || submitting}>取消</Button>
              <Button
                aria-label={buttonAriaLabel}
                className="min-w-48 flex-1"
                variant="primary"
                isDisabled={busy || submitting || !label.trim()}
                onPress={() => void submit()}
              >
                {buttonLabel}
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
      <Button className="hidden" aria-hidden="true">
        打开上游 CA 导入对话框
      </Button>
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
                选择签发上游 Server 证书的单个 CA 锚（.cer / .crt / .pem /
                .der）。不要选择证书链、带私钥的 Server 身份文件或 client.p12。
              </p>
              <p className="text-xs text-[var(--telemetry-muted)]">
                导入时会立即解析证书，并在当前页面显示主题、SAN、有效期和
                SHA-256，并在“测试 Server 连接”中实际验证。
              </p>
            </Modal.Body>
            <Modal.Footer>
              <Button slot="close" variant="outline" isDisabled={busy}>取消</Button>
              <Button
                variant="primary"
                isDisabled={busy || !label.trim()}
                onPress={() => void onImport()}
              >
                选择 CA 证书（.cer / .crt / .pem / .der）
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
