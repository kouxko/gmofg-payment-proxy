import type { UiTone } from "@/generated/rust-types";

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

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

export function formatDuration(milliseconds?: number | null): string {
  if (milliseconds == null) return "—";
  if (milliseconds < 1000) return `${milliseconds} ms`;
  return `${(milliseconds / 1000).toFixed(3)} s`;
}

export function formatTimestamp(value?: string | null): string {
  if (!value) return "—";
  return value.replace("T", " ").replace("Z", "").slice(0, 23);
}

export function percent(value: number, maximum: number): number {
  if (maximum <= 0) return 0;
  return Math.min(100, Math.round((value / maximum) * 100));
}
