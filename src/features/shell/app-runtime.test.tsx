// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { Link } from "@heroui/react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AppRuntime } from "./app-runtime";

const routerMocks = vi.hoisted(() => ({
  push: vi.fn(),
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: routerMocks.push,
  }),
}));

vi.mock("./app-shell", () => ({
  AppShell: ({ children }: { children: React.ReactNode }) => children,
}));

describe("AppRuntime client navigation", () => {
  it("routes HeroUI links through Next.js without document navigation", async () => {
    const user = userEvent.setup();
    render(
      <AppRuntime>
        <Link href="/rules">打开规则</Link>
      </AppRuntime>,
    );

    await user.click(screen.getByRole("link", { name: "打开规则" }));

    expect(routerMocks.push).toHaveBeenCalledWith("/rules");
    expect(window.location.pathname).toBe("/");
  });
});
