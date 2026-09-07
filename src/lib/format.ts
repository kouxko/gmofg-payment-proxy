import type { UiTone } from "@/generated/rust-types";

/**
 * 纯显示格式化工具。
 *
 * 这里不能判断业务成功/失败；Rust 已经把语义压缩为 UiTone、字节数、毫秒数等
 * ViewModel 字段，前端只把它们转换成 HeroUI 颜色和易读文本。
 */

export function toneColor(
  tone: UiTone,
): "default" | "accent" | "success" | "warning" | "danger" {
  switch (tone) {
    case "positive":
      return "success";
    case "warning":
      return "warning";
    case "danger":
      return "danger";
    case "info":
      return "accent";
    default:
      return "default";
  }
}

/** 使用二进制单位显示报文/容量大小，避免页面重复实现单位换算。 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

/** 将带时区的 ISO 时间戳转换为本机时间，并保留毫秒精度。 */
export function formatTimestamp(value?: string | null): string {
  if (!value) return "—";
  const timestamp = new Date(value);
  if (Number.isNaN(timestamp.getTime())) return "—";

  const pad = (part: number, length = 2) => String(part).padStart(length, "0");
  return `${pad(timestamp.getFullYear(), 4)}-${pad(timestamp.getMonth() + 1)}-${pad(timestamp.getDate())} ${pad(timestamp.getHours())}:${pad(timestamp.getMinutes())}:${pad(timestamp.getSeconds())}.${pad(timestamp.getMilliseconds(), 3)}`;
}
