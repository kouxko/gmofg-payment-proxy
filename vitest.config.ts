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
      reporter: ["text", "html"],
    },
  },
});
