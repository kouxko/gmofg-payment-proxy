import type { DocumentSchemaNode, ProtocolPackageSchemaViewModel } from "@/generated/rust-types";

export function isProtocolPackageSchema(value: unknown): value is ProtocolPackageSchemaViewModel {
  if (!isRecord(value) || !hasOnly(value, ["root"])) return false;
  return isDocumentSchemaNode(value.root);
}

export function schemaTitle(schema: ProtocolPackageSchemaViewModel): string {
  return schema.root.title?.trim() || "未命名 Schema";
}

export function schemaNodeCount(node: DocumentSchemaNode): number {
  if (node.type === "object") {
    return 1 + Object.values(node.properties).reduce((count, child) => count + schemaNodeCount(child), 0);
  }
  if (node.type === "array") return 1 + schemaNodeCount(node.items);
  return 1;
}

export function flattenSchema(node: DocumentSchemaNode, path = ""): Array<{
  path: string;
  title: string | null;
  type: DocumentSchemaNode["type"];
}> {
  const current = [{ path: path || "/", title: node.title ?? null, type: node.type }];
  if (node.type === "object") {
    return Object.entries(node.properties).reduce(
      (rows, [name, child]) => rows.concat(flattenSchema(child, `${path}/${escapePointerToken(name)}`)),
      current,
    );
  }
  if (node.type === "array") return current.concat(flattenSchema(node.items, `${path}/*`));
  return current;
}

function isDocumentSchemaNode(value: unknown): value is DocumentSchemaNode {
  if (!isRecord(value) || typeof value.type !== "string") return false;
  if (!(value.title === undefined || value.title === null || isVisibleText(value.title))) return false;
  if (value.type === "string" || value.type === "number" || value.type === "boolean") {
    return hasOnly(value, value.title === undefined ? ["type"] : ["type", "title"]);
  }
  if (value.type === "object") {
    if (!hasOnly(value, value.title === undefined ? ["type", "properties"] : ["type", "title", "properties"])
      || !isRecord(value.properties)) return false;
    return Object.values(value.properties).every((child) => isDocumentSchemaNode(child));
  }
  if (value.type === "array") {
    return hasOnly(value, value.title === undefined ? ["type", "items"] : ["type", "title", "items"])
      && isDocumentSchemaNode(value.items);
  }
  return false;
}

function escapePointerToken(value: string): string {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function isVisibleText(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0 && Array.from(value).length <= 128;
}

function hasOnly(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => actual.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
