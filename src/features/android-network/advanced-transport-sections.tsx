import type { ReactElement } from "react";
import { Button, Label, ListBox, Select } from "@heroui/react";
import type {
  BitCorruptionProfile,
  NthTcpFlagDrop,
  PacketDirection,
  PathMtuProfile,
  PmtuMode,
  TcpFlag,
} from "@/generated/rust-types";
import { NumericField } from "./android-network-fields";
import {
  PACKET_DIRECTIONS,
  PMTU_MODES,
  TCP_FLAGS,
} from "./android-network-types";
import type { UpdateWeakNetwork } from "./android-network-types";

interface TcpFlagDropsSectionProps {
  drops: NthTcpFlagDrop[];
  onUpdate: UpdateWeakNetwork;
  onAdd: () => void;
}

export function TcpFlagDropsSection({
  drops,
  onUpdate,
  onAdd,
}: TcpFlagDropsSectionProps): ReactElement {
  function updateDrop(index: number, changes: Partial<NthTcpFlagDrop>): void {
    onUpdate({
      nth_tcp_flag_drops: drops.map((drop, current) => (
        current === index ? { ...drop, ...changes } : drop
      )),
    });
  }

  return (
    <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="font-semibold">第 N 个 TCP 标志位丢弃</h3>
          <p className="text-sm text-[var(--telemetry-muted)]">每个方向与标志位独立计数。</p>
        </div>
        <Button
          variant="outline"
          onPress={onAdd}
        >
          添加 TCP 丢弃
        </Button>
      </div>
      {drops.length === 0 && (
        <p className="text-sm text-[var(--telemetry-muted)]">未配置定向 TCP 标志位丢弃。</p>
      )}
      {drops.map((drop, index) => (
        <div
          key={`tcp-drop-${index}`}
          className="grid grid-cols-[1fr_1fr_1fr_auto] items-end gap-3 max-[820px]:grid-cols-2 max-[620px]:grid-cols-1"
        >
          <Select
            aria-label={`TCP 丢弃 ${index + 1} 方向`}
            selectedKey={drop.direction}
            onSelectionChange={(direction) => updateDrop(index, {
              direction: direction as PacketDirection,
            })}
          >
            <Label>方向</Label>
            <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover>
              <ListBox>
                {PACKET_DIRECTIONS.map((item) => (
                  <ListBox.Item key={item.id} id={item.id}>{item.label}</ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <Select
            aria-label={`TCP 丢弃 ${index + 1} Flag`}
            selectedKey={drop.flag}
            onSelectionChange={(flag) => updateDrop(index, { flag: flag as TcpFlag })}
          >
            <Label>TCP 标志位</Label>
            <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover>
              <ListBox>
                {TCP_FLAGS.map((item) => (
                  <ListBox.Item key={item.id} id={item.id}>{item.label}</ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <NumericField
            label="第 N 个"
            value={drop.nth}
            onChange={(nth) => updateDrop(index, { nth })}
          />
          <Button
            variant="danger-soft"
            onPress={() => onUpdate({
              nth_tcp_flag_drops: drops.filter((_, current) => current !== index),
            })}
          >
            删除 TCP 丢弃
          </Button>
        </div>
      ))}
    </section>
  );
}

interface PathMtuSectionProps {
  pathMtu: PathMtuProfile;
  onUpdate: UpdateWeakNetwork;
}

export function PathMtuSection({
  pathMtu,
  onUpdate,
}: PathMtuSectionProps): ReactElement {
  return (
    <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-5">
      <div>
        <h3 className="font-semibold">MTU / MSS / PMTU</h3>
        <p className="text-sm text-[var(--telemetry-muted)]">0 表示不设置 MTU 或 MSS；保存时会校验具体组合。</p>
      </div>
      <div className="grid grid-cols-3 gap-4 max-[820px]:grid-cols-1">
        <Select
          aria-label="PMTU 处理模式"
          selectedKey={pathMtu.mode}
          onSelectionChange={(mode) => onUpdate({
            path_mtu: { ...pathMtu, mode: mode as PmtuMode },
          })}
        >
          <Label>PMTU 处理模式</Label>
          <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
          <Select.Popover>
            <ListBox>
              {PMTU_MODES.map((item) => (
                <ListBox.Item key={item.id} id={item.id}>{item.label}</ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
        <NumericField
          label="路径 MTU（0 为未设置）"
          value={pathMtu.mtu ?? 0}
          onChange={(value) => onUpdate({
            path_mtu: { ...pathMtu, mtu: nullableZero(value) },
          })}
        />
        <NumericField
          label="TCP 最大报文段限制（0 为未设置）"
          value={pathMtu.mss_clamp ?? 0}
          onChange={(value) => onUpdate({
            path_mtu: { ...pathMtu, mss_clamp: nullableZero(value) },
          })}
        />
      </div>
    </section>
  );
}

interface CorruptionSectionProps {
  corruption: BitCorruptionProfile;
  onUpdate: UpdateWeakNetwork;
}

export function CorruptionSection({
  corruption,
  onUpdate,
}: CorruptionSectionProps): ReactElement {
  return (
    <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-5">
      <div>
        <h3 className="font-semibold">TCP / UDP 数据载荷位翻转</h3>
        <p className="text-sm text-[var(--telemetry-muted)]">仅编辑概率与每包翻转位数，保存时统一判断可发送性。</p>
      </div>
      <div className="grid grid-cols-2 gap-4 max-[620px]:grid-cols-1">
        <NumericField
          label="位翻转概率（基点）"
          value={corruption.probability_basis_points}
          onChange={(value) => onUpdate({
            corruption: { ...corruption, probability_basis_points: value },
          })}
        />
        <NumericField
          label="每包翻转位数"
          value={corruption.bits_per_packet}
          onChange={(value) => onUpdate({
            corruption: { ...corruption, bits_per_packet: value },
          })}
        />
      </div>
    </section>
  );
}

function nullableZero(value: number): number | null {
  return value === 0 ? null : value;
}
