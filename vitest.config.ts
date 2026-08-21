import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // 完整 UI 测试会并行挂载多个 HeroUI 页面。CI 或本机高负载时，
    // jsdom 的用户交互可能超过 Vitest 默认的 5 秒，但单测本身并未死锁。
    // 统一设置上限，避免各测试文件重复声明不同的超时时间。
    testTimeout: 15_000,
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "json-summary"],
      thresholds: {
        // Current repository baseline. These values prevent coverage regression
        // without pretending that historical frontend code already meets the
        // stricter protocol-package target below.
        statements: 68,
        branches: 65,
        functions: 59,
        lines: 71,
        // New Socket protocol UI must meet the feature-level gate from its
        // first committed source file. Vitest ignores globs with no source
        // matches, so these become active as the directories are introduced.
        "src/features/protocol-packages/**": {
          statements: 90,
          branches: 90,
          functions: 90,
          lines: 90,
        },
        "src/features/protocol-packages/protocol-package-source.ts": {
          statements: 100,
          branches: 100,
          functions: 100,
          lines: 100,
        },
        "src/features/settings/external-package-service-settings.tsx": {
          statements: 90,
          branches: 90,
          functions: 90,
          lines: 90,
        },
        "src/features/listeners/socket-*": {
          statements: 90,
          branches: 90,
          functions: 90,
          lines: 90,
        },
        "src/features/rules/socket-rule*": {
          statements: 90,
          branches: 90,
          functions: 90,
          lines: 90,
        },
        "src/features/capture/socket-*": {
          statements: 90,
          branches: 90,
          functions: 90,
          lines: 90,
        },
      },
    },
  },
});
