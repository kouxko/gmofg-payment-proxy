// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CapturePageViewModel } from "@/generated/rust-types";
import { CaptureView } from "./capture-view";

const commandMocks = vi.hoisted(() => ({
  captureQuery: vi.fn(),
  captureGetDetail: vi.fn(),
  captureClearView: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
  useBootstrap: () => ({
    proxy: {
      state_text: "已停止",
      ui_tone: "neutral",
    },
  }),
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({
    navigate: vi.fn(),
  }),
}));

const emptyPage: CapturePageViewModel = {
  rows: [],
  total: 0,
  page: 1,
  page_size: 50,
  total_pages: 1,
  event_cursor: 0,
  oldest_event_id: null,
  runtime_epoch: null,
  snapshot_required: false,
  empty_message: "没有符合条件的抓包事件。",
};

describe("CAPTURE-002 exception filter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.captureQuery.mockReturnValue(
      Promise.resolve({ status: "ok", data: emptyPage }),
    );
  });

  it("keeps the visual control inside HeroUI's clickable checkbox content", async () => {
    const user = userEvent.setup();
    const { container } = render(<CaptureView initialPage={emptyPage} />);
    const checkbox = screen.getByRole("checkbox", { name: "仅看异常" });
    const control = container.querySelector(
      '[data-slot="checkbox-control"]',
    );

    expect(control).not.toBeNull();
    expect(checkbox).not.toBeChecked();

    expect(control!.parentElement).toHaveAttribute(
      "data-slot",
      "checkbox-content",
    );
    expect(control!.parentElement).toHaveTextContent("仅看异常");
    await user.click(checkbox);
    expect(checkbox).toBeChecked();
    await waitFor(() =>
      expect(commandMocks.captureQuery).toHaveBeenLastCalledWith(
        expect.objectContaining({ exceptions_only: true }),
      ),
    );
  });
});
