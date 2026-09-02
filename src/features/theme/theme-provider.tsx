"use client";

import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useSyncExternalStore,
} from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = Exclude<ThemePreference, "system">;

export const THEME_STORAGE_KEY = "intercept-proxy-theme";
const SYSTEM_THEME_QUERY = "(prefers-color-scheme: dark)";

const preferenceListeners = new Set<() => void>();
let memoryPreference: ThemePreference = "system";

function isThemePreference(value: string | null): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function readThemePreference(): ThemePreference {
  if (typeof window === "undefined") return "system";
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return isThemePreference(stored) ? stored : "system";
  } catch {
    return memoryPreference;
  }
}

function readServerPreference(): ThemePreference {
  return "system";
}

function subscribePreference(listener: () => void) {
  preferenceListeners.add(listener);
  const handleStorage = (event: StorageEvent) => {
    if (event.key === THEME_STORAGE_KEY) listener();
  };
  window.addEventListener("storage", handleStorage);
  return () => {
    preferenceListeners.delete(listener);
    window.removeEventListener("storage", handleStorage);
  };
}

function subscribeSystemTheme(listener: () => void) {
  const media = window.matchMedia(SYSTEM_THEME_QUERY);
  media.addEventListener("change", listener);
  return () => media.removeEventListener("change", listener);
}

function readSystemTheme(): ResolvedTheme {
  if (typeof window === "undefined") return "light";
  return window.matchMedia(SYSTEM_THEME_QUERY).matches ? "dark" : "light";
}

function readServerSystemTheme(): ResolvedTheme {
  return "light";
}

function applyTheme(theme: ResolvedTheme) {
  const root = document.documentElement;
  root.classList.remove("light", "dark");
  root.classList.add(theme);
  root.dataset.theme = theme;
}

type ThemeContextValue = {
  preference: ThemePreference;
  resolvedTheme: ResolvedTheme;
  setPreference: (preference: ThemePreference) => void;
};

const ThemeContext = createContext<ThemeContextValue>({
  preference: "system",
  resolvedTheme: "light",
  setPreference: () => undefined,
});

export function ThemeProvider({ children }: Readonly<{ children: React.ReactNode }>) {
  const preference = useSyncExternalStore(
    subscribePreference,
    readThemePreference,
    readServerPreference,
  );
  const systemTheme = useSyncExternalStore(
    subscribeSystemTheme,
    readSystemTheme,
    readServerSystemTheme,
  );
  const resolvedTheme = preference === "system" ? systemTheme : preference;

  useLayoutEffect(() => {
    applyTheme(resolvedTheme);
  }, [resolvedTheme]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      preference,
      resolvedTheme,
      setPreference(nextPreference) {
        memoryPreference = nextPreference;
        try {
          window.localStorage.setItem(THEME_STORAGE_KEY, nextPreference);
        } catch {
          // localStorage 不可用时仍允许当前页面切换主题。
        }
        preferenceListeners.forEach((listener) => listener());
      },
    }),
    [preference, resolvedTheme],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useAppTheme() {
  return useContext(ThemeContext);
}
