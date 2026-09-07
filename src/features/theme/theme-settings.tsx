"use client";

import { Button, Card, Chip } from "@heroui/react";
import type { ThemePreference } from "./theme-provider";
import { useAppTheme } from "./theme-provider";

const options: ReadonlyArray<{
  value: ThemePreference;
  label: string;
  description: string;
}> = [
  { value: "system", label: "跟随系统", description: "随操作系统外观自动切换" },
  { value: "light", label: "浅色", description: "始终使用浅色外观" },
  { value: "dark", label: "深色", description: "始终使用深色外观" },
];

export function ThemeSettings() {
  const { preference, resolvedTheme, setPreference } = useAppTheme();

  return (
    <Card className="border border-[var(--telemetry-line)] shadow-none">
      <div className="p-5">
        <div className="flex items-start gap-4">
          <div className="min-w-0">
            <h2 className="font-semibold">外观主题</h2>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            主题保存在本机，切换后立即生效，不会写入 Workspace 或应用设置。
          </p>
          </div>
          <Chip className="ml-auto shrink-0" size="sm" variant="soft">
            当前{resolvedTheme === "dark" ? "深色" : "浅色"}
          </Chip>
        </div>
        <div className="mt-5 grid grid-cols-3 gap-3 max-[760px]:grid-cols-1" role="group" aria-label="外观主题">
          {options.map((option) => {
            const selected = preference === option.value;
            return (
              <Button
                key={option.value}
                className="h-auto min-h-16 w-full items-start justify-start px-4 py-3 text-left"
                variant={selected ? "primary" : "outline"}
                aria-pressed={selected}
                onPress={() => setPreference(option.value)}
              >
                <span>
                  <span className="block font-medium">{option.label}</span>
                  <span className="mt-1 block text-xs opacity-75">{option.description}</span>
                </span>
              </Button>
            );
          })}
        </div>
      </div>
    </Card>
  );
}
