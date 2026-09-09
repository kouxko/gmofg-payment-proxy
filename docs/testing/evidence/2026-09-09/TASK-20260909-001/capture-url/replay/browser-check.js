async (page) => {
  await page.reload();
  const expected = `/api/pay?token=a%2Fb+c&empty=&long=${"x".repeat(2048)}`;
  await page.setViewportSize({ width: 1280, height: 800 });
  const grid = page.getByRole("grid", { name: "HTTP 与 Socket 运行记录" });
  const before = await grid.evaluate(el => el.getBoundingClientRect().width);
  await page.getByRole("button", { name: "追加 App 原始长请求" }).click();
  const path = page.getByTitle(expected, { exact: true });
  const results = [];
  for (const width of [1280, 960]) {
    await page.setViewportSize({ width, height: 800 });
    const value = await path.evaluate(el => {
      const style = getComputedStyle(el);
      return { text: el.textContent, title: el.title, width: el.clientWidth, scrollWidth: el.scrollWidth, height: el.getBoundingClientRect().height, lineHeight: style.lineHeight, whiteSpace: style.whiteSpace, textOverflow: style.textOverflow, overflow: style.overflow };
    });
    if (value.text !== expected || value.title !== expected) throw new Error("target changed");
    if (value.whiteSpace !== "nowrap" || value.textOverflow !== "ellipsis" || value.overflow !== "hidden") throw new Error("missing single-line ellipsis");
    if (!(value.scrollWidth > value.width && value.height <= parseFloat(value.lineHeight))) throw new Error("not a bounded single line");
    const gridWidth = await grid.evaluate(el => el.getBoundingClientRect().width);
    if (width === 1280 && gridWidth !== before) throw new Error("long URL expanded table");
    results.push({ viewport: width, ...value, gridWidth, before });
  }
  await page.setViewportSize({ width: 1280, height: 800 });
  await path.click();
  if (await page.getByRole("status", { name: "选中连接" }).textContent() !== "exchange-http-1") throw new Error("selection failed");
  await page.screenshot({ path: "output/playwright/capture-url.png", fullPage: true });
  return { result: "PASS", selection: "exchange-http-1", cases: results };
}
