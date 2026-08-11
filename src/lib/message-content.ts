import type {
  MessageContentKind,
  MessageContentViewModel,
} from "@/generated/rust-types";

type DisplayableMessage = Pick<
  MessageContentViewModel,
  "body_text" | "json"
>;

export type InspectableMessage = MessageContentViewModel;

export type QueryAwareRequest = Partial<{ query_string: string | null }>;

/**
 * 格式化 Rust 已解析的报文正文，仅改变界面展示。
 *
 * JSON 的解析和合法性判断仍由 Rust 完成。Rust 没有提供 JSON 结构时，
 * 直接显示原始正文，避免前端对 Shift-JIS、非法 JSON 或普通文本作业务推断。
 */
export function formatMessageBody(
  message: DisplayableMessage | null | undefined,
  emptyText: string,
): string {
  if (message?.json != null) {
    return JSON.stringify(message.json, null, 2);
  }

  return message?.body_text ?? emptyText;
}

export function messageMediaType(message?: InspectableMessage | null) {
  if (message?.media_type) return message.media_type;
  const contentType = Object.entries(message?.headers ?? {}).find(
    ([name]) => name.toLowerCase() === "content-type",
  )?.[1][0];
  return contentType?.split(";", 1)[0]?.trim() || undefined;
}

export function messageCharset(message?: InspectableMessage | null) {
  if (message?.charset) return message.charset;
  const contentType = Object.entries(message?.headers ?? {}).find(
    ([name]) => name.toLowerCase() === "content-type",
  )?.[1][0];
  return contentType?.match(/(?:^|;)\s*charset\s*=\s*["']?([^;"']+)/i)?.[1]?.trim();
}

export function messageContentKind(
  message?: InspectableMessage | null,
): Exclude<MessageContentKind, "unknown"> {
  const declared = message?.content_kind?.toLowerCase();
  if (declared === "json" || declared === "xml" || declared === "text") {
    return declared;
  }
  if (declared === "binary" || declared === "raw") return "binary";

  const mediaType = messageMediaType(message)?.toLowerCase();
  if (mediaType?.includes("json") || message?.json != null) return "json";
  if (mediaType?.includes("xml")) return "xml";
  if (message?.body_text != null) return "text";
  return "binary";
}

export function messageContentLabel(message?: InspectableMessage | null) {
  const kind = messageContentKind(message);
  if (kind === "json") return "JSON";
  if (kind === "xml") return "XML";
  if (kind === "text") return "文本";
  return "二进制";
}

export function requestQueryString(
  request: QueryAwareRequest | null | undefined,
  target: string,
) {
  if (request?.query_string != null) return request.query_string;
  const separator = target.indexOf("?");
  return separator < 0 ? undefined : target.slice(separator + 1);
}
