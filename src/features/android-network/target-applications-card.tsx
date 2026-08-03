import type { ReactElement } from "react";
import {
  Button,
  Card,
  Input,
  Label,
  Table,
} from "@heroui/react";
import { CircleCheckFill } from "@gravity-ui/icons";
import type { AndroidPackageViewModel } from "@/generated/rust-types";

interface TargetApplicationsCardProps {
  visiblePackages: AndroidPackageViewModel[];
  selectedPackages: Set<string>;
  filterDraft: string;
  activeFilter: string;
  selectedSerial?: string | null;
  filtering: boolean;
  onFilterDraftChange: (value: string) => void;
  onApplyFilter: () => void;
  onClearFilter: () => void;
  onTogglePackage: (item: AndroidPackageViewModel, enabled: boolean) => void;
}

export function TargetApplicationsCard({
  visiblePackages,
  selectedPackages,
  filterDraft,
  activeFilter,
  selectedSerial,
  filtering,
  onFilterDraftChange,
  onApplyFilter,
  onClearFilter,
  onTogglePackage,
}: TargetApplicationsCardProps): ReactElement {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>目标应用</Card.Title>
        <Card.Description>
          点击应用所在整行即可选择，再次点击取消；共享 UID 的整组扩选、签名快照和确认由 Rust 自动完成。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-3 p-4">
        <div className="grid grid-cols-[minmax(0,1fr)_auto_auto] items-end gap-2 max-[620px]:grid-cols-1">
          <div className="grid gap-1">
            <Label>按包名筛选</Label>
            <Input
              aria-label="包名筛选"
              value={filterDraft}
              onChange={(event) => onFilterDraftChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") onApplyFilter();
              }}
              placeholder="例如 com.example.client"
            />
          </div>
          <Button
            variant="primary"
            isDisabled={!selectedSerial || filtering}
            onPress={onApplyFilter}
          >
            筛选
          </Button>
          <Button
            variant="outline"
            isDisabled={!activeFilter && !filterDraft}
            onPress={onClearFilter}
          >
            清除
          </Button>
        </div>

        <Table>
          <Table.ScrollContainer className="h-80 overflow-y-auto overscroll-contain [scrollbar-gutter:stable]">
            <Table.Content aria-label="安卓应用列表">
              <Table.Header>
                <Table.Column isRowHeader>包名</Table.Column>
                <Table.Column>UID</Table.Column>
                <Table.Column>签名</Table.Column>
              </Table.Header>
              <Table.Body renderEmptyState={() => (
                <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">
                  {emptyStateText(filtering, activeFilter)}
                </div>
              )}>
                {visiblePackages.map((item) => (
                    <PackageRow
                      key={item.package_name}
                      item={item}
                      selected={selectedPackages.has(item.package_name)}
                      onToggle={onTogglePackage}
                    />
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      </Card.Content>
    </Card>
  );
}

interface PackageRowProps {
  item: AndroidPackageViewModel;
  selected: boolean;
  onToggle: (item: AndroidPackageViewModel, enabled: boolean) => void;
}

function PackageRow({ item, selected, onToggle }: PackageRowProps): ReactElement {
  const selectedCellClass = selected ? "bg-[var(--telemetry-accent-soft)]" : undefined;
  const enabled = Boolean(item.signing_sha256);

  return (
    <Table.Row
      id={item.package_name}
      textValue={item.package_name}
      isDisabled={!enabled}
      className={enabled ? "cursor-pointer" : "cursor-not-allowed opacity-60"}
      onAction={() => onToggle(item, !selected)}
    >
      <Table.Cell className={`${selectedCellClass ?? ""} font-mono text-xs`}>
        <span className="flex min-w-0 items-center gap-2">
          <span className="grid size-5 shrink-0 place-items-center" aria-hidden={!selected}>
            {selected && (
              <CircleCheckFill
                aria-label="已选中"
                className="size-5 text-[var(--telemetry-accent)]"
              />
            )}
          </span>
          <span className="truncate">{item.package_name}</span>
        </span>
      </Table.Cell>
      <Table.Cell className={selectedCellClass}>
        {item.uid}{item.shared_uid !== null ? "（共享）" : ""}
      </Table.Cell>
      <Table.Cell className={`${selectedCellClass ?? ""} max-w-48 truncate font-mono text-xs`}>
        {item.signing_sha256 ?? "无法读取"}
      </Table.Cell>
    </Table.Row>
  );
}

function emptyStateText(filtering: boolean, activeFilter: string): string {
  if (filtering) return "正在由 Rust 筛选包名…";
  if (activeFilter) return "没有匹配该包名的应用。";
  return "选择设备后读取应用。";
}
