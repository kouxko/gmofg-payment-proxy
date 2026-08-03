import type { ReactElement } from "react";
import { Alert, Button, Card, Input, Label, Switch } from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidNetworkProfileSummary,
} from "@/generated/rust-types";

interface ProfileSelectorCardProps {
  profiles: AndroidNetworkProfileSummary[];
  selectedProfileId?: string;
  loading: boolean;
  busy: boolean;
  onNew: () => void;
  onOpen: (profileId: string) => void;
}

export function ProfileSelectorCard({
  profiles,
  selectedProfileId,
  loading,
  busy,
  onNew,
  onOpen,
}: ProfileSelectorCardProps): ReactElement {
  return (
    <Card className="h-fit border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header className="flex-row items-center justify-between gap-3">
        <div>
          <Card.Title>设备网络方案</Card.Title>
          <Card.Description>保存目标应用、透明代理路由与可选弱网参数。</Card.Description>
        </div>
        <Button size="sm" variant="primary" isDisabled={busy} onPress={onNew}>
          新建
        </Button>
      </Card.Header>
      <Card.Content className="flex flex-wrap gap-2 p-4">
        {profiles.map((profile) => (
          <Button
            key={profile.id}
            variant={selectedProfileId === profile.id ? "primary" : "outline"}
            isDisabled={busy}
            onPress={() => onOpen(profile.id)}
          >
            {profile.name}
          </Button>
        ))}
        {!loading && profiles.length === 0 && (
          <p className="py-2 text-sm text-[var(--telemetry-muted)]">还没有保存的设备网络方案。</p>
        )}
      </Card.Content>
    </Card>
  );
}

interface ProfileBasicsCardProps {
  draft: AndroidNetworkProfile;
  onChange: (draft: AndroidNetworkProfile) => void;
}

export function ProfileBasicsCard({
  draft,
  onChange,
}: ProfileBasicsCardProps): ReactElement {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>方案基本信息</Card.Title>
        <Card.Description className="font-mono text-xs">{draft.id}</Card.Description>
      </Card.Header>
      <Card.Content className="grid grid-cols-2 gap-4 p-4 max-[760px]:grid-cols-1">
        <div className="grid gap-1">
          <Label>方案名称</Label>
          <Input
            aria-label="设备网络方案名称"
            value={draft.name}
            onChange={(event) => onChange({ ...draft, name: event.target.value })}
          />
        </div>
        <div className="flex items-end">
          <Switch
            isSelected={draft.auto_resume_after_reboot}
            onChange={(autoResume) => onChange({
              ...draft,
              auto_resume_after_reboot: autoResume,
            })}
          >
            <Switch.Content>
              <Switch.Control><Switch.Thumb /></Switch.Control>
              <span>解锁且网络可用后自动恢复</span>
            </Switch.Content>
          </Switch>
        </div>
      </Card.Content>
    </Card>
  );
}

interface ProfileActionsProps {
  busy: boolean;
  selectedSerial?: string | null;
  dangerousConfirmed: boolean;
  onDangerousConfirmedChange: (confirmed: boolean) => void;
  onSave: () => void;
  onStart: () => void;
  onApply: () => void;
  onStop: () => void;
}

export function ProfileActions({
  busy,
  selectedSerial,
  dangerousConfirmed,
  onDangerousConfirmedChange,
  onSave,
  onStart,
  onApply,
  onStop,
}: ProfileActionsProps): ReactElement {
  return (
    <div className="flex flex-wrap items-center gap-2 pb-5">
      <Switch isSelected={dangerousConfirmed} onChange={onDangerousConfirmedChange}>
        <Switch.Content>
          <Switch.Control><Switch.Thumb /></Switch.Control>
          <span>确认 100% 丢包或黑洞风险</span>
        </Switch.Content>
      </Switch>
      <Button variant="outline" isDisabled={busy} onPress={onSave}>保存方案</Button>
      <Button variant="primary" isDisabled={busy || !selectedSerial} onPress={onStart}>启动</Button>
      <Button variant="outline" isDisabled={busy || !selectedSerial} onPress={onApply}>应用修改</Button>
      <Button variant="danger-soft" isDisabled={busy || !selectedSerial} onPress={onStop}>停止网络接管</Button>
    </div>
  );
}

interface EmptyProfileStateProps {
  busy: boolean;
  onNew: () => void;
}

export function EmptyProfileState({ busy, onNew }: EmptyProfileStateProps): ReactElement {
  return (
    <Alert status="accent">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>尚未选择设备网络方案</Alert.Title>
        <Alert.Description>
          新建或选择方案后，即可配置目标应用、代理路由、弱网覆盖范围和 TCP/IP 参数。
        </Alert.Description>
        <div className="mt-3">
          <Button size="sm" variant="primary" isDisabled={busy} onPress={onNew}>
            新建设备网络方案
          </Button>
        </div>
      </Alert.Content>
    </Alert>
  );
}
