import type { ReactNode } from "react";
import { Button, Card } from "@heroui/react";
import { TrashBin } from "@gravity-ui/icons";
import type { ProxyWorkspace } from "@/generated/rust-types";
import type {
  ComponentKind,
  ComponentOperation,
} from "./workspace-components-editor-model";

export type WorkspaceComponentsSectionProps = {
  workspace: ProxyWorkspace;
  onChange: (workspace: ProxyWorkspace) => void;
  onAdd: (kind: ComponentKind) => void;
  onIntent: (
    kind: ComponentKind,
    id: string,
    operation: ComponentOperation,
    value: string,
  ) => void;
  disabled: boolean;
};

export function ComponentCard({
  title,
  index,
  id,
  onDelete,
  disabled,
  trailing,
  children,
}: {
  title: string;
  index: number;
  id: string;
  onDelete: () => void;
  disabled: boolean;
  trailing?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card>
      <Card.Content className="grid grid-cols-2 gap-3 p-4 max-[700px]:grid-cols-1">
        <div className="col-span-2 flex items-center gap-3 max-[700px]:col-span-1">
          <strong>
            {title} {index + 1}
          </strong>
          <code className="text-xs text-[var(--telemetry-muted)]">{id}</code>
          {trailing}
          <Button
            className={trailing ? undefined : "ml-auto"}
            isIconOnly
            isDisabled={disabled}
            aria-label={`删除${title} ${index + 1}`}
            variant="danger-soft"
            onPress={onDelete}
          >
            <TrashBin className="size-4" />
          </Button>
        </div>
        {children}
      </Card.Content>
    </Card>
  );
}
