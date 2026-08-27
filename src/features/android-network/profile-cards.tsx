import type { ReactElement } from "react";
import { Alert, Button, Card, Chip, Input, Label, Switch } from "@heroui/react";
import type {
  AndroidNetworkProfile,
  AndroidNetworkProfileSummary,
  AndroidRuntimeOwnerViewModel,
} from "@/generated/rust-types";

interface ProfileSelectorCardProps {
  profiles: AndroidNetworkProfileSummary[];
  selectedProfileId?: string;
  activeProfileId?: string;
  vpnStateText?: string;
  loading: boolean;
  busy: boolean;
  onNew: () => void;
  onOpen: (profileId: string) => void;
}

export function ProfileSelectorCard({
  profiles,
  selectedProfileId,
  activeProfileId,
  vpnStateText,
  loading,
  busy,
  onNew,
  onOpen,
}: ProfileSelectorCardProps): ReactElement {
  const isEmpty = !loading && profiles.length === 0;
  const activeProfileIsOutsideCurrentWorkspace = Boolean(
    activeProfileId && !profiles.some((profile) => profile.id === activeProfileId),
  );

  return (
    <Card className="h-fit border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header className="flex-row items-center justify-between gap-3 pb-2">
        <div>
          <Card.Title>设备网络方案</Card.Title>
          <Card.Description>保存目标应用、透明代理路由与可选弱网参数。</Card.Description>
        </div>
        {!isEmpty && (
          <Button size="sm" variant="primary" isDisabled={busy} onPress={onNew}>
            新建
          </Button>
        )}
      </Card.Header>
      <Card.Content className="p-4 pt-0">
        {activeProfileIsOutsideCurrentWorkspace && (
          <Alert status="accent" className="mb-3">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>其他 Workspace 的方案正在运行</Alert.Title>
              <Alert.Description>
                切换 Workspace 不会停止设备网络接管；运行方案仍使用其原 Workspace 的代理入口。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        {isEmpty ? (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-[var(--telemetry-panel-muted)] px-4 py-3">
            <p className="text-sm text-[var(--telemetry-muted)]">还没有保存的设备网络方案。</p>
            <Button size="sm" variant="primary" isDisabled={busy} onPress={onNew}>
              新建设备网络方案
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2">
            {profiles.map((profile) => {
              const selected = selectedProfileId === profile.id;
              const active = activeProfileId === profile.id;
              return (
                <Button
                  key={profile.id}
                  className="h-auto w-full justify-between gap-3 py-3 text-left"
                  variant={selected ? "primary" : "outline"}
                  isDisabled={busy}
                  onPress={() => onOpen(profile.id)}
                >
                  <span className="min-w-0 truncate text-left">{profile.name}</span>
                  <span className="flex shrink-0 items-center gap-2">
                    <span className="text-xs opacity-70">{profile.target_count} 个应用</span>
                    {active && (
                      <Chip size="sm" color="success" variant="soft">
                        正在执行{vpnStateText ? ` · ${vpnStateText}` : ""}
                      </Chip>
                    )}
                  </span>
                </Button>
              );
            })}
          </div>
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
  runtimeOwner?: AndroidRuntimeOwnerViewModel;
  runtimeOwnerCount: number;
  runtimeOwnerReady: boolean;
  dangerousConfirmed: boolean;
  onDangerousConfirmedChange: (confirmed: boolean) => void;
  onSave: () => void;
  onStart: () => void;
  onApply: () => void;
}

export function ProfileActions({
  busy,
  selectedSerial,
  runtimeOwner,
  runtimeOwnerCount,
  runtimeOwnerReady,
  dangerousConfirmed,
  onDangerousConfirmedChange,
  onSave,
  onStart,
  onApply,
}: ProfileActionsProps): ReactElement {
  const ownerCapacityReached = runtimeOwnerCount >= 8 && !runtimeOwner;
  return (
    <div className="flex flex-wrap items-center gap-2 pb-5">
      <Switch isSelected={dangerousConfirmed} onChange={onDangerousConfirmedChange}>
        <Switch.Content>
          <Switch.Control><Switch.Thumb /></Switch.Control>
          <span>确认 100% 丢包或黑洞风险</span>
        </Switch.Content>
      </Switch>
      <Button variant="outline" isDisabled={busy || !selectedSerial} onPress={onSave}>保存方案</Button>
      <Button
        variant="primary"
        isDisabled={busy || !selectedSerial || !runtimeOwnerReady || Boolean(runtimeOwner) || ownerCapacityReached}
        onPress={onStart}
      >启动</Button>
      <Button
        variant="outline"
        isDisabled={busy || !selectedSerial || !runtimeOwnerReady || !runtimeOwner}
        onPress={onApply}
      >应用修改</Button>
      {ownerCapacityReached && (
        <p className="basis-full text-xs text-[var(--telemetry-warning)]">
          已达到 8 台运行设备上限；请先停止一台设备，再启动新的设备。
        </p>
      )}
      {!runtimeOwnerReady && (
        <p className="basis-full text-xs text-[var(--telemetry-warning)]">
          正在确认实际运行设备；确认完成前不能启动或应用方案。
        </p>
      )}
    </div>
  );
}

export function UnselectedProfileState(): ReactElement {
  return (
    <Alert status="accent">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>尚未选择设备网络方案</Alert.Title>
        <Alert.Description>
          请选择上方已有方案，或使用右上角“新建”创建方案。
        </Alert.Description>
      </Alert.Content>
    </Alert>
  );
}
