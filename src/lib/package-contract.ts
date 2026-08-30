import type {
  FrameResult,
  PackageManifest,
  PackageRegisterNotification,
  PackageRpcRequest,
} from "@/generated/rust-types";
import { PACKAGE_CONTRACT_VALIDATION } from "@/generated/rust-types";

type JsonObject = Record<string, unknown>;
type ResultGuard = (value: unknown) => boolean;

const PACKAGE_ID = new RegExp(PACKAGE_CONTRACT_VALIDATION.packageIdPattern, "u");
const PACKAGE_VERSION = new RegExp(PACKAGE_CONTRACT_VALIDATION.packageVersionPattern, "u");
const STABLE_ERROR_CODES = new Set<string>(PACKAGE_CONTRACT_VALIDATION.stableErrorCodes);

function validPackageVersion(value: string): boolean {
  if (!PACKAGE_VERSION.test(value)
    || value.length > PACKAGE_CONTRACT_VALIDATION.packageVersionMaxBytes) return false;
  const maximum = BigInt(PACKAGE_CONTRACT_VALIDATION.packageVersionCoreNumericMax);
  return value.split(/[+-]/u, 1)[0].split(".").every((part) => BigInt(part) <= maximum);
}

function object(value: unknown): JsonObject | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as JsonObject
    : undefined;
}

function exact(value: JsonObject, keys: readonly string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key))
    && keys.every((key) => key in value);
}

function isSchema(value: unknown): boolean {
  const node = object(value);
  if (!node || typeof node.type !== "string") return false;
  const titleValid = node.title === undefined || (typeof node.title === "string"
    && node.title.trim().length > 0
    && Array.from(node.title).length <= PACKAGE_CONTRACT_VALIDATION.schemaTitleMaxChars);
  if (!titleValid) return false;
  if (["string", "number", "boolean"].includes(node.type)) {
    return Object.keys(node).every((key) => key === "type" || key === "title");
  }
  if (node.type === "object") {
    const properties = object(node.properties);
    return properties !== undefined
      && Object.keys(node).every((key) => ["type", "title", "properties"].includes(key))
      && Object.values(properties).every(isSchema);
  }
  return node.type === "array"
    && Object.keys(node).every((key) => ["type", "title", "items"].includes(key))
    && isSchema(node.items);
}

function isDirection(value: unknown): boolean {
  const direction = object(value);
  return direction !== undefined
    && Object.keys(direction).every((key) => key === "schema")
    && (direction.schema === undefined || isSchema(direction.schema));
}

export function isPackageManifest(value: unknown): value is PackageManifest {
  const manifest = object(value);
  if (!manifest || !exact(manifest, ["api", "kind", "package", "document"])) return false;
  const packageMetadata = object(manifest.package);
  const document = object(manifest.document);
  if (manifest.api !== 1 || (manifest.kind !== "http" && manifest.kind !== "socket")) return false;
  if (!packageMetadata || !exact(packageMetadata, ["id", "version", "name", "description"])) return false;
  if (![packageMetadata.id, packageMetadata.version, packageMetadata.name, packageMetadata.description]
    .every((field) => typeof field === "string")) return false;
  const id = packageMetadata.id as string;
  const version = packageMetadata.version as string;
  const name = packageMetadata.name as string;
  if (!PACKAGE_ID.test(id) || id.length > PACKAGE_CONTRACT_VALIDATION.packageIdMaxBytes) return false;
  if (!validPackageVersion(version)) return false;
  if (name.trim().length === 0) return false;
  if (!document || !exact(document, ["upstream", "downstream"])) return false;
  if (!isDirection(document.upstream) || !isDirection(document.downstream)) return false;
  if (manifest.kind === "socket") {
    return object(document.upstream)?.schema !== undefined
      && object(document.downstream)?.schema !== undefined;
  }
  return true;
}

export function isPackageRegisterNotification(value: unknown): value is PackageRegisterNotification {
  const notification = object(value);
  return notification !== undefined
    && exact(notification, ["jsonrpc", "method", "params"])
    && notification.jsonrpc === "2.0"
    && notification.method === "package.register"
    && isPackageManifest(notification.params);
}

function isDocument(value: unknown): boolean {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value)
    && (!Number.isInteger(value) || Number.isSafeInteger(value));
  if (Array.isArray(value)) return value.every(isDocument);
  const record = object(value);
  return record !== undefined && Object.values(record).every(isDocument);
}

export function isPackageDocument(value: unknown): boolean {
  return isDocument(value);
}

export function isPackageRpcRequest(value: unknown): value is PackageRpcRequest {
  const request = object(value);
  if (!request || !exact(request, ["jsonrpc", "id", "method", "params"])) return false;
  if (request.jsonrpc !== "2.0" || typeof request.id !== "string") return false;
  const params = object(request.params);
  if (!params || typeof request.method !== "string") return false;
  if (request.method === "hooks.upstream.frame" || request.method === "hooks.downstream.frame") {
    return exact(params, ["buffer"]) && typeof params.buffer === "string"
      && /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(params.buffer);
  }
  if (request.method === "hooks.upstream.decode" || request.method === "hooks.downstream.decode") {
    return exact(params, ["input"]) && typeof params.input === "string";
  }
  if (request.method === "hooks.upstream.encode" || request.method === "hooks.downstream.encode") {
    return exact(params, ["originalInput", "document"])
      && typeof params.originalInput === "string" && isDocument(params.document);
  }
  if (request.method === "document.upstream.display" || request.method === "document.downstream.display") {
    return exact(params, ["document"]) && isDocument(params.document);
  }
  return false;
}

export function isFrameResult(value: unknown): value is FrameResult {
  const result = object(value);
  if (!result || typeof result.status !== "string") return false;
  if (result.status === "need_more") {
    return Object.keys(result).every((key) => key === "status" || key === "requiredBytes")
      && (result.requiredBytes === undefined
        || (Number.isSafeInteger(result.requiredBytes) && Number(result.requiredBytes) >= 0));
  }
  if (result.status === "complete") {
    return exact(result, ["status", "consumedBytes"])
      && Number.isSafeInteger(result.consumedBytes) && Number(result.consumedBytes) > 0;
  }
  return result.status === "reject" && exact(result, ["status", "reason"])
    && typeof result.reason === "string";
}

export function isPackageRpcSuccess(value: unknown, resultGuard: ResultGuard): boolean {
  const response = object(value);
  return response !== undefined
    && exact(response, ["jsonrpc", "id", "result"])
    && response.jsonrpc === "2.0"
    && typeof response.id === "string"
    && resultGuard(response.result);
}

export function isPackageRpcFailure(value: unknown): boolean {
  const response = object(value);
  if (!response || !exact(response, ["jsonrpc", "id", "error"])
    || response.jsonrpc !== "2.0" || typeof response.id !== "string") return false;
  const error = object(response.error);
  if (!error || !exact(error, ["code", "message", "data"])
    || !Number.isSafeInteger(error.code) || typeof error.message !== "string") return false;
  const data = object(error.data);
  return data !== undefined && exact(data, ["code"])
    && typeof data.code === "string" && STABLE_ERROR_CODES.has(data.code);
}
