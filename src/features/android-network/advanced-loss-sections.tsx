import type { ReactElement } from "react";
import { Button, Switch } from "@heroui/react";
import type {
  BlackoutWindow,
  BurstLossProfile,
} from "@/generated/rust-types";
import { NumericField } from "./android-network-fields";
import type { UpdateWeakNetwork } from "./android-network-types";

interface BurstLossSectionProps {
  enabled: boolean;
  burstLoss: BurstLossProfile | null;
  onUpdate: UpdateWeakNetwork;
  onEnabledChange: (enabled: boolean) => void;
}

export function BurstLossSection({
  enabled,
  burstLoss,
  onUpdate,
  onEnabledChange,
}: BurstLossSectionProps): ReactElement {
  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="font-semibold">Gilbert-Elliott 连续丢包</h3>
          <p className="text-sm text-[var(--telemetry-muted)]">以好/坏两种状态模拟突发连续丢包。</p>
        </div>
        <Switch
          isSelected={enabled}
          onChange={onEnabledChange}
        >
          <Switch.Content>
            <Switch.Control><Switch.Thumb /></Switch.Control>
            <span>启用连续丢包</span>
          </Switch.Content>
        </Switch>
      </div>
      {enabled && burstLoss && (
        <div className="grid grid-cols-4 gap-4 max-[1000px]:grid-cols-2 max-[620px]:grid-cols-1">
          <NumericField
            label="进入坏状态（基点）"
            value={burstLoss.enter_bad_state_basis_points}
            onChange={(value) => onUpdate({
              burst_loss: { ...burstLoss, enter_bad_state_basis_points: value },
            })}
          />
          <NumericField
            label="离开坏状态（基点）"
            value={burstLoss.leave_bad_state_basis_points}
            onChange={(value) => onUpdate({
              burst_loss: { ...burstLoss, leave_bad_state_basis_points: value },
            })}
          />
          <NumericField
            label="好状态丢包（基点）"
            value={burstLoss.good_state_loss_basis_points}
            onChange={(value) => onUpdate({
              burst_loss: { ...burstLoss, good_state_loss_basis_points: value },
            })}
          />
          <NumericField
            label="坏状态丢包（基点）"
            value={burstLoss.bad_state_loss_basis_points}
            onChange={(value) => onUpdate({
              burst_loss: { ...burstLoss, bad_state_loss_basis_points: value },
            })}
          />
        </div>
      )}
    </section>
  );
}

interface BlackoutWindowsSectionProps {
  windows: BlackoutWindow[];
  onUpdate: UpdateWeakNetwork;
  onAdd: () => void;
}

export function BlackoutWindowsSection({
  windows,
  onUpdate,
  onAdd,
}: BlackoutWindowsSectionProps): ReactElement {
  function updateWindow(index: number, changes: Partial<BlackoutWindow>): void {
    onUpdate({
      blackout_windows: windows.map((window, current) => (
        current === index ? { ...window, ...changes } : window
      )),
    });
  }

  return (
    <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="font-semibold">断网时间窗口</h3>
          <p className="text-sm text-[var(--telemetry-muted)]">时间均相对于本次弱网引擎启动时刻。</p>
        </div>
        <Button
          variant="outline"
          onPress={onAdd}
        >
          添加断网窗口
        </Button>
      </div>
      {windows.length === 0 && (
        <p className="text-sm text-[var(--telemetry-muted)]">未配置断网窗口。</p>
      )}
      {windows.map((window, index) => (
        <div
          key={`blackout-${index}`}
          className="grid grid-cols-[1fr_1fr_auto] items-end gap-3 max-[700px]:grid-cols-1"
        >
          <NumericField
            label={`窗口 ${index + 1} · 启动后开始（ms）`}
            value={window.start_after_millis}
            onChange={(value) => updateWindow(index, { start_after_millis: value })}
          />
          <NumericField
            label="持续时间（ms）"
            value={window.duration_millis}
            onChange={(value) => updateWindow(index, { duration_millis: value })}
          />
          <Button
            variant="danger-soft"
            onPress={() => onUpdate({
              blackout_windows: windows.filter((_, current) => current !== index),
            })}
          >
            删除窗口
          </Button>
        </div>
      ))}
    </section>
  );
}
