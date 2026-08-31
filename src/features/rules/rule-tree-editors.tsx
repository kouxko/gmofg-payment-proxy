import { useState } from "react";
import { Button } from "@heroui/react";
import type {
  Condition,
  ConditionTree,
  UnifiedAction,
} from "@/generated/rust-types";
import type { DocumentSchemaField } from "./rule-document-schema";

type MetadataNode = {
  name: string;
  path: string;
  field?: MetadataField;
  readOnly?: boolean;
  children: Map<string, MetadataNode>;
};

type MetadataField = Pick<DocumentSchemaField, "name" | "label" | "type" | "itemTemplate">;

export function DocumentMetadataTree({
  fields,
  condition,
  readonlyPaths = new Set(fields.map((field) => field.name)),
  localFields = fields.filter((field) => !readonlyPaths.has(field.name)),
}: {
  fields: DocumentSchemaField[];
  localFields?: MetadataField[];
  condition: ConditionTree;
  readonlyPaths?: ReadonlySet<string>;
}) {
  const counts = documentConditionCounts(condition);
  const schemaFields = fields.filter((field) => readonlyPaths.has(field.name));
  const schemaRoot = buildMetadataTree(schemaFields, true);
  const localRoot = buildMetadataTree(localFields, false);
  const hasSchemaMetadata = schemaRoot.field != null || schemaRoot.children.size > 0;
  const hasLocalMetadata = localRoot.field != null || localRoot.children.size > 0;
  return (
    <section className="space-y-2" aria-labelledby="document-metadata-heading">
      <h5 className="text-sm font-medium" id="document-metadata-heading">Document metadata tree</h5>
      {!hasSchemaMetadata && !hasLocalMetadata ? (
        <p className="text-xs text-[var(--telemetry-muted)]">当前规则没有 Schema 路径；新路径只由用户明确输入后加入。</p>
      ) : <div className="space-y-2">
        {hasSchemaMetadata && <MetadataTree label="Schema metadata tree" root={schemaRoot} counts={counts} />}
        {hasLocalMetadata && <MetadataTree label="Rule-local metadata tree" root={localRoot} counts={counts} />}
      </div>}
    </section>
  );
}

function MetadataTree({ label, root, counts }: { label: string; root: MetadataNode; counts: Map<string, number> }) {
  return <ul aria-label={label} className="space-y-1" role="tree">
    {root.field
      ? <MetadataTreeNode counts={counts} node={root} />
      : [...root.children.values()].map((node) => <MetadataTreeNode counts={counts} key={node.path} node={node} />)}
  </ul>;
}

function MetadataTreeNode({ node, counts }: { node: MetadataNode; counts: Map<string, number> }) {
  const numeric = /^\d+$/.test(node.name);
  const label = node.field?.itemTemplate ? "Array items template (*)" : node.field?.label ?? (numeric ? `Array index ${node.name}` : node.name);
  const type = node.field?.type;
  return (
    <li aria-label={`${label} ${type ?? "group"}${node.readOnly ? " 只读" : ""}`} aria-selected="false" data-readonly={node.readOnly ? "true" : "false"} role="treeitem">
      <div className="flex items-center gap-2 rounded-md border border-[var(--telemetry-line)] px-2 py-1 text-xs">
        <code>{label}</code>
        {type && <span>{type}</span>}
        <span className="ml-auto">条件 {counts.get(node.path) ?? 0}</span>
      </div>
      {node.children.size > 0 && (
        <ul className="ml-4 mt-1 space-y-1" role="group">
          {[...node.children.values()].map((child) => <MetadataTreeNode counts={counts} key={child.path} node={child} />)}
        </ul>
      )}
    </li>
  );
}

export function ConditionTreeEditor({
  tree,
  onChange,
  onInsertRequest,
}: {
  tree: ConditionTree;
  onChange: (tree: ConditionTree) => void;
  onInsertRequest?: (targetPath: number[], subgroup: "all" | "any" | null) => void;
}) {
  const [targetPath, setTargetPath] = useState<number[]>([]);
  return (
    <section className="space-y-2" aria-labelledby="condition-tree-heading">
      <h5 className="text-sm font-medium" id="condition-tree-heading">递归条件树</h5>
      <ConditionNode
        node={tree}
        onChange={onChange}
        onInsertRequest={onInsertRequest}
        onTargetPathChange={setTargetPath}
        path={[]}
        targetPath={targetPath}
      />
    </section>
  );
}

function ConditionNode({
  node,
  onChange,
  onInsertRequest,
  onTargetPathChange,
  path,
  targetPath,
}: {
  node: ConditionTree;
  onChange: (node: ConditionTree) => void;
  onInsertRequest?: (targetPath: number[], subgroup: "all" | "any" | null) => void;
  onTargetPathChange: (path: number[]) => void;
  path: number[];
  targetPath: number[];
}) {
  if (node.operator === "leaf") {
    return (
      <div className="rounded-md border border-[var(--telemetry-line)] p-2 text-xs">
        <span>{conditionLabel(node.children)}</span>
        <div className="mt-2 flex gap-1">
          <Button size="sm" variant="ghost" onPress={() => onChange({ operator: "all", children: [node] })}>用 AND 包裹</Button>
          <Button size="sm" variant="ghost" onPress={() => onChange({ operator: "any", children: [node] })}>用 OR 包裹</Button>
        </div>
      </div>
    );
  }
  const groupLabel = node.operator === "all" ? "AND 条件组" : "OR 条件组";
  const groupName = path.length === 0 ? `${groupLabel} 根` : `${groupLabel} ${path.map((index) => index + 1).join(".")}`;
  const selected = samePath(path, targetPath);
  return (
    <fieldset aria-label={groupName} className="space-y-2 rounded-md border border-[var(--telemetry-line)] p-2" data-insertion-target={selected ? "true" : "false"}>
      <legend className="px-1 text-xs font-medium">{groupLabel}</legend>
      {onInsertRequest && <Button aria-label={`选择 ${groupName} 为添加目标`} size="sm" variant="ghost" onPress={() => onTargetPathChange(path)}>{selected ? "当前添加目标" : "选择为添加目标"}</Button>}
      {onInsertRequest && selected && <div className="flex flex-wrap gap-1">
        <Button size="sm" variant="outline" onPress={() => onInsertRequest(path, null)}>在目标组添加条件</Button>
        <Button size="sm" variant="outline" onPress={() => onInsertRequest(path, "all")}>在目标组添加 AND 子组</Button>
        <Button size="sm" variant="outline" onPress={() => onInsertRequest(path, "any")}>在目标组添加 OR 子组</Button>
      </div>}
      <Button size="sm" variant="ghost" onPress={() => onChange({ ...node, operator: node.operator === "all" ? "any" : "all" })}>
        切换为 {node.operator === "all" ? "OR" : "AND"}
      </Button>
      <div className="space-y-2 pl-3">
        {node.children.map((child, index) => (
          <div className="flex items-start gap-1" key={index}>
            <div className="min-w-0 flex-1"><ConditionNode node={child} onChange={(next) => onChange({ ...node, children: node.children.map((item, itemIndex) => itemIndex === index ? next : item) })} onInsertRequest={onInsertRequest} onTargetPathChange={onTargetPathChange} path={[...path, index]} targetPath={targetPath} /></div>
            {node.children.length > 1 && <Button aria-label={`删除条件节点 ${index + 1}`} size="sm" variant="ghost" onPress={() => onChange({ ...node, children: node.children.filter((_, itemIndex) => itemIndex !== index) })}>删除</Button>}
          </div>
        ))}
      </div>
    </fieldset>
  );
}

function samePath(left: number[], right: number[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export function OrderedActionList({
  actions,
  label,
  onChange,
}: {
  actions: UnifiedAction[];
  label: (action: UnifiedAction) => string;
  onChange: (actions: UnifiedAction[]) => void;
}) {
  return (
    <section className="space-y-2" aria-labelledby="ordered-actions-heading">
      <h5 className="text-sm font-medium" id="ordered-actions-heading">有序动作列表</h5>
      <ol className="space-y-2">
        {actions.map((action, index) => (
          <li className="flex items-center gap-2 rounded-md border border-[var(--telemetry-line)] p-2 text-xs" key={index}>
            <span>{index + 1}. {label(action)}</span>
            <div className="ml-auto flex gap-1">
              <Button aria-label={`上移动作 ${index + 1}`} isDisabled={index === 0} size="sm" variant="ghost" onPress={() => onChange(move(actions, index, index - 1))}>上移</Button>
              <Button aria-label={`下移动作 ${index + 1}`} isDisabled={index === actions.length - 1} size="sm" variant="ghost" onPress={() => onChange(move(actions, index, index + 1))}>下移</Button>
              <Button aria-label={`删除动作 ${index + 1}`} isDisabled={actions.length === 1} size="sm" variant="ghost" onPress={() => onChange(actions.filter((_, itemIndex) => itemIndex !== index))}>删除</Button>
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}

function buildMetadataTree(fields: MetadataField[], readOnly: boolean): MetadataNode {
  const root: MetadataNode = { name: "", path: "", children: new Map() };
  for (const field of fields) {
    const tokens = pointerTokens(field.name);
    let current = root;
    let path = "";
    for (const token of tokens) {
      path += `/${escapeToken(token)}`;
      let child = current.children.get(token);
      if (!child) {
        child = { name: token, path, readOnly, children: new Map() };
        current.children.set(token, child);
      }
      current = child;
    }
    current.field = field;
    current.readOnly = readOnly;
  }
  return root;
}

function documentConditionCounts(tree: ConditionTree): Map<string, number> {
  const counts = new Map<string, number>();
  visitConditions(tree, (condition) => {
    if (condition.source === "document" || condition.source === "document_pattern") counts.set(condition.path, (counts.get(condition.path) ?? 0) + 1);
  });
  return counts;
}

function visitConditions(tree: ConditionTree, visit: (condition: Condition) => void) {
  if (tree.operator === "leaf") visit(tree.children);
  else tree.children.forEach((child) => visitConditions(child, visit));
}

function conditionLabel(condition: Condition): string {
  if (condition.source === "document" || condition.source === "document_pattern") return `${condition.path || "/"} · ${condition.predicate.type}`;
  if (condition.source === "nth_hit") return `第 ${condition.count} 次命中`;
  return "HTTP 字段条件";
}

export function pointerTokens(pointer: string): string[] {
  if (pointer === "") return [];
  return pointer.slice(1).split("/").map((token) => token.replaceAll("~1", "/").replaceAll("~0", "~"));
}

function escapeToken(token: string): string {
  return token.replaceAll("~", "~0").replaceAll("/", "~1");
}

function move<T>(items: T[], from: number, to: number): T[] {
  const result = [...items];
  const [item] = result.splice(from, 1);
  result.splice(to, 0, item);
  return result;
}
