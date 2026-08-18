import type { ProtocolPackageSchemaViewModel } from "@/generated/rust-types";

export function isProtocolPackageSchema(value: unknown): value is ProtocolPackageSchemaViewModel {
  if (!isRecord(value)
    || !hasOnly(value, ["id", "version", "title", "fields"])
    || !isBoundedIdentity(value.id)
    || typeof value.version !== "number"
    || !Number.isSafeInteger(value.version)
    || value.version < 1
    || !isVisibleText(value.title)
    || !Array.isArray(value.fields)
    || value.fields.length === 0
    || value.fields.length > 256) return false;

  const names = new Set<string>();
  for (const field of value.fields) {
    if (!isRecord(field)
      || !hasOnly(field, ["name", "type", "label"])
      || !isBoundedIdentity(field.name)
      || names.has(field.name)
      || typeof field.type !== "string"
      || !["string", "int", "bool", "blob"].includes(field.type)
      || !isVisibleText(field.label)) return false;
    names.add(field.name);
  }
  return true;
}

function isBoundedIdentity(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 64;
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
