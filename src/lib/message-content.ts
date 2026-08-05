import type { MessageContentViewModel } from "@/generated/rust-types";

type DisplayableMessage = Pick<
  MessageContentViewModel,
  "body_text" | "json"
>;

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
