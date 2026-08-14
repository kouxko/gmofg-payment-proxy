// @vitest-environment jsdom

/** 验证内存工作区路由解析、查询参数和未知路径回退。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  useWorkspaceNavigation,
  WorkspaceNavigationProvider,
} from "./workspace-navigation";

function NavigationProbe() {
  const { pathname, searchParams, navigate } = useWorkspaceNavigation();
  return (
    <>
      <output>
        {pathname}:{searchParams.get("sessionId") ?? "none"}
      </output>
      <button onClick={() => navigate("/rules?sessionId=session-1")}>
        打开规则
      </button>
      <button onClick={() => navigate("/listeners")}>打开监听器</button>
      <button onClick={() => navigate("/protocol-packages")}>打开协议包</button>
    </>
  );
}

describe("persistent desktop workspace navigation", () => {
  it("changes the active view without navigating the WebView document", async () => {
    const user = userEvent.setup();
    const documentUrl = window.location.href;
    render(
      <WorkspaceNavigationProvider>
        <NavigationProbe />
      </WorkspaceNavigationProvider>,
    );

    await user.click(screen.getByRole("button", { name: "打开规则" }));

    expect(screen.getByText("/rules:session-1")).toBeInTheDocument();
    expect(window.location.href).toBe(documentUrl);

    await user.click(screen.getByRole("button", { name: "打开监听器" }));
    expect(screen.getByText("/listeners:none")).toBeInTheDocument();
    expect(window.location.href).toBe(documentUrl);

    await user.click(screen.getByRole("button", { name: "打开协议包" }));
    expect(screen.getByText("/protocol-packages:none")).toBeInTheDocument();
    expect(window.location.href).toBe(documentUrl);
  });
});
