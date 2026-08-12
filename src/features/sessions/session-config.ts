import {
  parseAbsoluteToLocal,
  parseDateTime,
  toCalendarDateTime,
  type DateValue,
} from "@internationalized/date";
import type { SessionQuery } from "@/generated/rust-types";

export const defaultSessionQuery: SessionQuery = {
  keyword: null,
  terminal_ip: null,
  channel: null,
  result: null,
  rule_id: null,
  started_from: null,
  started_to: null,
  sort: "started_at",
  direction: "desc",
};

export const sessionDetailTabLabels = {
  overview: "概览",
  request: "请求",
  response: "响应",
} as const;

export function sessionFilterDateValue(value: string | null): DateValue | null {
  if (!value) return null;

  try {
    return parseAbsoluteToLocal(value);
  } catch {
    try {
      return parseDateTime(value);
    } catch {
      return null;
    }
  }
}

export function sessionFilterDateText(value: DateValue | null): string | null {
  if (!value) return null;
  return toCalendarDateTime(value).toString().slice(0, 16);
}
