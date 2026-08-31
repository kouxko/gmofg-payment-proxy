import { fireEvent, screen } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";

export async function selectHttpMethodEquals(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /HTTP 匹配字段/ }));
  await user.click(await screen.findByRole("option", { name: "Method" }));
  await user.click(screen.getByRole("button", { name: /HTTP 匹配操作符/ }));
  await user.click(await screen.findByRole("option", { name: "equals" }));
}

export async function selectHttpAction(user: ReturnType<typeof userEvent.setup>, label: string, parameters: string) {
  await user.click(screen.getByRole("button", { name: /HTTP 动作类型/ }));
  await user.click(await screen.findByRole("option", { name: label }));
  fireEvent.change(screen.getByRole("textbox", { name: "动作参数 JSON" }), { target: { value: parameters } });
  await user.click(screen.getByRole("button", { name: "创建 HTTP 动作" }));
}
