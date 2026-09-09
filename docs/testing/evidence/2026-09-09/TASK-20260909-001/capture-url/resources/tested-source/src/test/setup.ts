import "@testing-library/jest-dom/vitest";

/**
 * Vitest 的统一浏览器测试环境清理。
 * 每个用例后卸载 React 树，避免前一页面的 HeroUI Overlay 或事件监听污染下一例。
 */
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
});

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}

Object.defineProperty(globalThis, "ResizeObserver", {
  configurable: true,
  value: ResizeObserverStub,
});

Object.defineProperty(window, "matchMedia", {
  configurable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }),
});

Object.defineProperty(Element.prototype, "getAnimations", {
  configurable: true,
  value: () => [],
});
