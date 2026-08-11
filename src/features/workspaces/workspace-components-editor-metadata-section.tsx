import { Button, Input, Label, ListBox, Select, Tabs } from "@heroui/react";
import { Plus } from "@gravity-ui/icons";
import {
  extractorSourceValue,
  updateAtIndex,
  updateExtractorSource,
} from "./workspace-components-editor-model";
import {
  ComponentCard,
  type WorkspaceComponentsSectionProps,
} from "./workspace-components-editor-section";

const extractorSourceOptions = [
  { id: "header", label: "Header" },
  { id: "json_path", label: "JSONPath" },
  { id: "body_text", label: "Body 文本" },
  { id: "fixed_value", label: "固定值" },
] as const;

export function MetadataExtractorsSection({
  workspace,
  onChange,
  onAdd,
  onIntent,
  disabled,
}: WorkspaceComponentsSectionProps) {
  return (
    <Tabs.Panel id="extractors" className="space-y-3 pt-4">
      <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("metadata_extractor")}>
        <Plus className="size-4" />
        新增提取器
      </Button>
      {workspace.metadata_extractors.map((extractor, index) => (
        <ComponentCard
          key={extractor.id}
          title="提取器"
          index={index}
          id={extractor.id}
          disabled={disabled}
          onDelete={() => onIntent("metadata_extractor", extractor.id, "delete", "")}
        >
          <div className="grid gap-1">
            <Label>名称（作为元数据 Key）</Label>
            <Input
              disabled={disabled}
              value={extractor.name}
              onChange={(event) =>
                onChange({
                  ...workspace,
                  metadata_extractors: updateAtIndex(
                    workspace.metadata_extractors,
                    index,
                    (item) => ({ ...item, name: event.target.value }),
                  ),
                })
              }
            />
          </div>
          <div className="grid gap-1">
            <Label>代理入口 ID（逗号分隔）</Label>
            <Input
              disabled={disabled}
              key={`${extractor.id}:${extractor.listener_ids.join(",")}`}
              defaultValue={extractor.listener_ids.join(", ")}
              onBlur={(event) =>
                onIntent("metadata_extractor", extractor.id, "listener_ids", event.target.value)
              }
            />
          </div>
          <Select
            isDisabled={disabled}
            aria-label={`提取器 ${index + 1} 来源`}
            selectedKey={extractor.source.kind}
            onSelectionChange={(key) =>
              onIntent("metadata_extractor", extractor.id, "variant", String(key))
            }
          >
            <Label>来源</Label>
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {extractorSourceOptions.map((option) => (
                  <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
                    {option.label}
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <div className="grid gap-1">
            <Label>参数</Label>
            <Input
              disabled={disabled || extractor.source.kind === "body_text"}
              value={extractorSourceValue(extractor.source)}
              onChange={(event) =>
                onChange({
                  ...workspace,
                  metadata_extractors: updateAtIndex(
                    workspace.metadata_extractors,
                    index,
                    (item) => ({
                      ...item,
                      source: updateExtractorSource(item.source, event.target.value),
                    }),
                  ),
                })
              }
              placeholder="Header 名 / $.path / 固定值"
            />
          </div>
        </ComponentCard>
      ))}
    </Tabs.Panel>
  );
}
