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

/** 将 Rust 的毫秒耗时显示为 ms 或秒；空值表示该阶段尚未发生。 */
export function formatDuration(milliseconds?: number | null): string {
  if (milliseconds == null) return "—";
  if (milliseconds < 1000) return `${milliseconds} ms`;
  return `${(milliseconds / 1000).toFixed(3)} s`;
}

/** 将 ISO 时间戳裁剪为表格需要的毫秒精度，不改变其业务含义。 */
export function formatTimestamp(value?: string | null): string {
  if (!value) return "—";
  return value.replace("T", " ").replace("Z", "").slice(0, 23);
}

/** 计算进度条百分比并限制在 0~100，maximum 无效时安全返回 0。 */
export function percent(value: number, maximum: number): number {
  if (maximum <= 0) return 0;
  return Math.min(100, Math.round((value / maximum) * 100));
}
