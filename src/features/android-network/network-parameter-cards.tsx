import type { ReactElement } from "react";
import { Alert, Button, Card, Input, Label, Switch } from "@heroui/react";
import type {
  AndroidDestinationTarget,
  AndroidNetworkProfile,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import { NumericField } from "./android-network-fields";
import type { UpdateWeakNetwork } from "./android-network-types";

interface DestinationTargetsCardProps {
  draft: AndroidNetworkProfile;
  onChange: (draft: AndroidNetworkProfile) => void;
}

export function DestinationTargetsCard({
  draft,
  onChange,
}: DestinationTargetsCardProps): ReactElement {
  const targets = draft.destination_targets ?? [];

  function updateTarget(index: number, changes: Partial<AndroidDestinationTarget>): void {
    onChange({
      ...draft,
      destination_targets: targets.map((target, current) => (
        current === index ? { ...target, ...changes } : target
      )),
    });
  }

  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>弱网覆盖范围（可选）</Card.Title>
        <Card.Description>
          这里只限制哪些连接实施弱网，不改变请求去向。留空表示覆盖所选应用访问的全部原始地址。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-3 p-4">
        {targets.length === 0 && (
          <Alert status="accent">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>当前对全部地址实施弱网</Alert.Title>
              <Alert.Description>
                应用访问多个 HTTP/HTTPS 地址时无需逐个添加，代理会保留每条连接的原始目标。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        {targets.map((target, index) => (
          <div
            key={`destination-${index}`}
            className="grid grid-cols-[minmax(0,1fr)_220px_auto] items-end gap-3 max-[760px]:grid-cols-1"
          >
            <div className="grid gap-1">
              <Label>地址 {index + 1}（IP 或 CIDR）</Label>
              <Input
                aria-label={`目标地址 ${index + 1}`}
                value={target.cidr}
                onChange={(event) => updateTarget(index, { cidr: event.target.value })}
                placeholder="10.0.34.50 或 10.0.34.0/24"
              />
            </div>
            <NumericField
              ariaLabel={`目标地址 ${index + 1} 端口`}
              label="端口（0 为全部）"
              minValue={0}
              maxValue={65_535}
              value={target.ports[0] ?? 0}
              onChange={(port) => updateTarget(index, { ports: port === 0 ? [] : [port] })}
            />
            <Button
              variant="danger-soft"
              onPress={() => onChange({
                ...draft,
                destination_targets: targets.filter((_, current) => current !== index),
              })}
            >
              删除地址
            </Button>
          </div>
        ))}
        <Button
          variant="outline"
          onPress={() => onChange({
            ...draft,
            destination_targets: [...targets, { cidr: "", ports: [] }],
          })}
        >
          添加弱网覆盖地址
        </Button>
        <p className="text-xs text-[var(--telemetry-muted)]">
          TUN 只能稳定识别 IP/CIDR；保存时会统一校验合法性、重复项和范围。
        </p>
      </Card.Content>
    </Card>
  );
}

interface BasicNetworkParametersCardProps {
  weak: WeakNetworkProfile;
  onUpdate: UpdateWeakNetwork;
}

export function BasicNetworkParametersCard({
  weak,
  onUpdate,
}: BasicNetworkParametersCardProps): ReactElement {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>TCP/IP 弱网参数</Card.Title>
        <Card.Description>概率使用 0–10000 基点，保存时统一校验。</Card.Description>
      </Card.Header>
      <Card.Content className="grid grid-cols-3 gap-4 p-4 max-[900px]:grid-cols-2 max-[620px]:grid-cols-1">
        <NumericField label="随机种子" value={weak.seed} onChange={(seed) => onUpdate({ seed })} />
        <NumericField ariaLabel="固定延迟" label="固定延迟（ms）" value={weak.fixed_delay_millis} onChange={(fixedDelay) => onUpdate({ fixed_delay_millis: fixedDelay })} />
        <NumericField ariaLabel="均匀抖动" label="均匀抖动（ms）" value={weak.uniform_jitter_millis} onChange={(jitter) => onUpdate({ uniform_jitter_millis: jitter })} />
        <NumericField ariaLabel="随机丢包" label="随机丢包（基点）" value={weak.random_loss_basis_points} onChange={(loss) => onUpdate({ random_loss_basis_points: loss })} />
        <NumericField label="重复包（基点）" value={weak.duplicate_basis_points} onChange={(duplicate) => onUpdate({ duplicate_basis_points: duplicate })} />
        <NumericField label="乱序（基点）" value={weak.reorder_basis_points} onChange={(reorder) => onUpdate({ reorder_basis_points: reorder })} />
        <NumericField ariaLabel="乱序保持时间" label="乱序保持时间（ms）" value={weak.maximum_reorder_hold_millis} onChange={(hold) => onUpdate({ maximum_reorder_hold_millis: hold })} />
        <NumericField label="上行 B/s（0 为不限）" value={weak.upload_bytes_per_second ?? 0} onChange={(value) => onUpdate({ upload_bytes_per_second: nullableZero(value) })} />
        <NumericField label="下行 B/s（0 为不限）" value={weak.download_bytes_per_second ?? 0} onChange={(value) => onUpdate({ download_bytes_per_second: nullableZero(value) })} />
        <div className="flex items-end">
          <Switch isSelected={weak.dns_blackhole} onChange={(dnsBlackhole) => onUpdate({ dns_blackhole: dnsBlackhole })}>
            <Switch.Content>
              <Switch.Control><Switch.Thumb /></Switch.Control>
              <span>DNS 53/853 黑洞</span>
            </Switch.Content>
          </Switch>
        </div>
      </Card.Content>
    </Card>
  );
}

function nullableZero(value: number): number | null {
  return value === 0 ? null : value;
}
