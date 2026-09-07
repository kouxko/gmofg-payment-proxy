import { useState, type ReactElement } from "react";
import { Alert, Button, Card, Input, Label, Switch } from "@heroui/react";
import type {
  AndroidDestinationTarget,
  AndroidNetworkProfile,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import { NumericField } from "./android-network-fields";
import type { UpdateWeakNetwork } from "./android-network-types";
import {
  matchingWeakNetworkScene,
  WEAK_NETWORK_SCENES,
} from "./weak-network-scenes";

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
  const weakSignature = commonWeakNetworkSignature(weak);
  const [customSignature, setCustomSignature] = useState<string | null>(
    () => matchingWeakNetworkScene(weak) ? null : weakSignature,
  );
  const matchingScene = matchingWeakNetworkScene(weak);
  const selectedScene = customSignature === weakSignature ? "custom" : matchingScene?.id ?? "custom";
  const selectedSceneConfig = WEAK_NETWORK_SCENES.find((scene) => scene.id === selectedScene);

  function updateCustom(changes: Partial<WeakNetworkProfile>): void {
    setCustomSignature(commonWeakNetworkSignature({ ...weak, ...changes }));
    onUpdate(changes);
  }

  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>常用弱网效果</Card.Title>
        <Card.Description>
          弱网可以单独运行，无需配置代理入口。设置常用效果后即可保存并启动。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-4 p-4">
        <div className="space-y-2">
          <p className="text-sm font-medium">快速场景</p>
          <div className="flex flex-wrap gap-2" role="group" aria-label="弱网快速场景">
            <Button
              size="sm"
              variant={selectedScene === "custom" ? "primary" : "outline"}
              aria-pressed={selectedScene === "custom"}
              onPress={() => setCustomSignature(weakSignature)}
            >
              自定义
            </Button>
            {WEAK_NETWORK_SCENES.map((scene) => (
              <Button
                key={scene.id}
                size="sm"
                variant={selectedScene === scene.id ? "primary" : "outline"}
                aria-pressed={selectedScene === scene.id}
                onPress={() => {
                  setCustomSignature(null);
                  onUpdate(scene.settings);
                }}
              >
                {scene.label}
              </Button>
            ))}
          </div>
          <p className="text-xs text-[var(--telemetry-muted)]">
            {selectedScene === "custom"
              ? "当前为自定义参数。"
              : `${selectedSceneConfig?.detail}；来源：${selectedSceneConfig?.sourceLabel}；RTT 已换算为单向延迟。`}
          </p>
        </div>
        <div className="grid grid-cols-3 gap-4 max-[900px]:grid-cols-2 max-[620px]:grid-cols-1">
          <NumericField ariaLabel="延迟（ms）" label="延迟（ms）" value={weak.fixed_delay_millis} onChange={(fixedDelay) => updateCustom({ fixed_delay_millis: fixedDelay })} />
          <NumericField ariaLabel="延迟波动（ms）" label="延迟波动（ms）" value={weak.uniform_jitter_millis} onChange={(jitter) => updateCustom({ uniform_jitter_millis: jitter })} />
          <NumericField
            ariaLabel="丢包率（%）"
            label="丢包率（%）"
            minValue={0}
            maxValue={100}
            step={0.01}
            value={basisPointsToPercent(weak.random_loss_basis_points)}
            onChange={(loss) => updateCustom({ random_loss_basis_points: percentToBasisPoints(loss) })}
          />
          <NumericField ariaLabel="上传限速（B/s，0 为不限）" label="上传限速（B/s，0 为不限）" value={weak.upload_bytes_per_second ?? 0} onChange={(value) => updateCustom({ upload_bytes_per_second: nullableZero(value) })} />
          <NumericField ariaLabel="下载限速（B/s，0 为不限）" label="下载限速（B/s，0 为不限）" value={weak.download_bytes_per_second ?? 0} onChange={(value) => updateCustom({ download_bytes_per_second: nullableZero(value) })} />
        </div>
      </Card.Content>
    </Card>
  );
}

export function ExpertNetworkParametersCard({
  weak,
  onUpdate,
}: BasicNetworkParametersCardProps): ReactElement {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>专家弱网参数</Card.Title>
        <Card.Description>用于复现确定性、重复包、乱序和 DNS 故障；保存时仍由 Rust 统一校验。</Card.Description>
      </Card.Header>
      <Card.Content className="grid grid-cols-3 gap-4 p-4 max-[900px]:grid-cols-2 max-[620px]:grid-cols-1">
        <NumericField ariaLabel="随机种子" label="随机种子" value={weak.seed} onChange={(seed) => onUpdate({ seed })} />
        <NumericField
          label="重复包率（%）"
          minValue={0}
          maxValue={100}
          step={0.01}
          value={basisPointsToPercent(weak.duplicate_basis_points)}
          onChange={(duplicate) => onUpdate({ duplicate_basis_points: percentToBasisPoints(duplicate) })}
        />
        <NumericField
          label="乱序率（%）"
          minValue={0}
          maxValue={100}
          step={0.01}
          value={basisPointsToPercent(weak.reorder_basis_points)}
          onChange={(reorder) => onUpdate({ reorder_basis_points: percentToBasisPoints(reorder) })}
        />
        <NumericField ariaLabel="乱序保持时间" label="乱序保持时间（ms）" value={weak.maximum_reorder_hold_millis} onChange={(hold) => onUpdate({ maximum_reorder_hold_millis: hold })} />
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

function basisPointsToPercent(value: number): number {
  return value / 100;
}

function percentToBasisPoints(value: number): number {
  return Math.round(value * 100);
}

function commonWeakNetworkSignature(weak: WeakNetworkProfile): string {
  return JSON.stringify([
    weak.fixed_delay_millis,
    weak.uniform_jitter_millis,
    weak.random_loss_basis_points,
    weak.upload_bytes_per_second,
    weak.download_bytes_per_second,
  ]);
}
