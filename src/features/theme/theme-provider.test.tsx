// @vitest-environment jsdom

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeSettings } from "./theme-settings";
import {
  THEME_STORAGE_KEY,
  ThemeProvider,
  useAppTheme,
} from "./theme-provider";

type MediaListener = () => void;
let systemDark = false;
const mediaListeners = new Set<MediaListener>();

function installLocalStorage() {
  const values = new Map<string, string>();
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: {
      clear: () => values.clear(),
      getItem: (key: string) => values.get(key) ?? null,
      key: (index: number) => [...values.keys()][index] ?? null,
      get length() {
        return values.size;
      },
      removeItem: (key: string) => values.delete(key),
      setItem: (key: string, value: string) => values.set(key, value),
    } satisfies Storage,
  });
}

function installMatchMedia() {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === "(prefers-color-scheme: dark)" && systemDark,
      media: query,
      onchange: null,
      addEventListener: (_type: string, listener: MediaListener) => mediaListeners.add(listener),
      removeEventListener: (_type: string, listener: MediaListener) => mediaListeners.delete(listener),
      dispatchEvent: () => false,
    })),
  });
}

function ThemeProbe() {
  const { preference, resolvedTheme } = useAppTheme();
  return <output>{`${preference}:${resolvedTheme}`}</output>;
}

describe("ThemeProvider", () => {
  beforeEach(() => {
    installLocalStorage();
    window.localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
    systemDark = false;
    mediaListeners.clear();
    installMatchMedia();
  });

  afterEach(() => {
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
  });

  it("defaults to the system theme and reacts to operating system changes", () => {
    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>,
    );

    expect(screen.getByText("system:light")).toBeVisible();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");

    act(() => {
      systemDark = true;
      mediaListeners.forEach((listener) => listener());
    });

    expect(screen.getByText("system:dark")).toBeVisible();
    expect(document.documentElement).toHaveClass("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("persists an explicit choice and applies it without reloading", async () => {
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <ThemeSettings />
      </ThemeProvider>,
    );

    await user.click(screen.getByRole("button", { name: /深色/ }));

    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(screen.getByRole("button", { name: /深色/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("restores a saved preference", () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");

    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>,
    );

    expect(screen.getByText("dark:dark")).toBeVisible();
    expect(document.documentElement).toHaveClass("dark");
  });
});
