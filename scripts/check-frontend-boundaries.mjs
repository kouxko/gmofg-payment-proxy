import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const sourceRoot = join(root, "src");

function sourceFiles(directory) {
  return readdirSync(directory)
    .flatMap((name) => {
      const path = join(directory, name);
      return statSync(path).isDirectory() ? sourceFiles(path) : [path];
    })
    .filter((path) => /\.(ts|tsx)$/.test(path))
    .filter((path) => !path.endsWith("generated/rust-types.ts"));
}

const forbidden = [
  [/\bfetch\s*\(/, "前端不得直接发起网络请求"],
  [/\bWebSocket\b/, "前端不得创建 WebSocket"],
  [
    /\b(?:localStorage|sessionStorage|indexedDB|caches)\b|\bdocument\.cookie\b/,
    "前端不得持久化业务数据",
  ],
  [
    /(?:from\s+|import\s*\(|require\s*\()\s*["'](?:node:)?(?:fs|path|crypto|net|tls)["']/,
    "展示层不得引入 Node.js 系统 API",
  ],
  [/<select(?:\s|>)|<option(?:\s|>)/, "UI 必须使用 HeroUI Select"],
  [/\bTextEncoder\b|\bTextDecoder\b/, "编码转换必须由 Rust 完成"],
  [
    /<(?:Modal|AlertDialog|Drawer)\.CloseTrigger(?:\s[^>]*)?>[\s\S]*?<\/(?:Modal|AlertDialog|Drawer)\.CloseTrigger>/,
    "CloseTrigger 仅用于右上角关闭图标；Footer 取消按钮必须使用 Button slot=\"close\"",
  ],
  [
    /<(?:Modal|AlertDialog|Drawer|Tooltip)\.Trigger(?:\s|>)/,
    "HeroUI v3 Overlay/Tooltip 必须直接使用 Button 作为触发器，禁止 Trigger 再包装交互控件",
  ],
];

const failures = [];
for (const file of sourceFiles(sourceRoot)) {
  const content = readFileSync(file, "utf8");
  const isTestFile = /\.(?:test|spec)\.(?:ts|tsx)$/.test(file);
  for (const [pattern, message] of forbidden) {
    if (pattern.test(content)) {
      failures.push(`${relative(root, file)}: ${message}`);
    }
  }
  if (
    !isTestFile &&
    /<(?:button|input|textarea|select|option)(?:\s|>)/.test(content)
  ) {
    failures.push(
      `${relative(root, file)}: 生产 UI 控件必须使用 HeroUI 组件`,
    );
  }
  if (!isTestFile && /\btype=["'](?:date|datetime-local|time)["']/.test(content)) {
    failures.push(
      `${relative(root, file)}: 日期与时间必须使用 HeroUI DatePicker、DateField 或 TimeField`,
    );
  }

  const overlayFooterPattern =
    /<(Modal|AlertDialog|Drawer)\.Footer(?:\s[^>]*)?>([\s\S]*?)<\/\1\.Footer>/g;
  for (const match of content.matchAll(overlayFooterPattern)) {
    const footer = match[2];
    if (
      /取消|关闭/.test(footer) &&
      !/<Button(?=[^>]*\bslot=["']close["'])[^>]*>/.test(footer)
    ) {
      failures.push(
        `${relative(root, file)}: Overlay Footer 的取消/关闭按钮必须使用 Button slot="close"`,
      );
    }
  }
}

const captureSource = readFileSync(
  join(sourceRoot, "features/capture/capture-view.tsx"),
  "utf8",
);
if (/commands\.ruleSave/.test(captureSource)) {
  failures.push(
    "src/features/capture/capture-view.tsx: CAPTURE-009 禁止在跳转前保存规则",
  );
}

const ruleEditorSource = readFileSync(
  join(sourceRoot, "features/rules/rule-editor.tsx"),
  "utf8",
);
if (
  /\bcreateAction\b|\bcreateCondition\b|\bparseBytes\b/.test(ruleEditorSource)
) {
  failures.push(
    "src/features/rules/rule-editor.tsx: 规则默认值和字节解析必须由 Rust 提供",
  );
}
for (const command of [
  "commands.ruleConditionDraft",
  "commands.ruleActionDraft",
  "commands.ruleMatchFieldDraft",
  "commands.ruleMatchOperatorDraft",
  "commands.ruleParseByteInput",
  "commands.ruleParseHeaderInput",
]) {
  if (!ruleEditorSource.includes(command)) {
    failures.push(
      `src/features/rules/rule-editor.tsx: 缺少 Rust 边界调用 ${command}`,
    );
  }
}
if (/\.split\(\s*["']\\n["']\s*\)/.test(ruleEditorSource)) {
  failures.push(
    "src/features/rules/rule-editor.tsx: Header 文本解析必须由 Rust 提供",
  );
}
if (
  /type\s*===\s*["']json_path["'][\s\S]{0,120}path:\s*["']\$["']/.test(
    ruleEditorSource,
  ) ||
  /type\s*===\s*["']regex["'][\s\S]{0,120}pattern:\s*["']["']/.test(
    ruleEditorSource,
  )
) {
  failures.push(
    "src/features/rules/rule-editor.tsx: 匹配字段和操作符默认值必须由 Rust draft Command 提供",
  );
}
if (
  !ruleEditorSource.includes("useAsyncRequestSlots") ||
  !ruleEditorSource.includes("function ConditionRow") ||
  !ruleEditorSource.includes("current.type === \"mock_response\"") ||
  !ruleEditorSource.includes("current.actions.map")
) {
  failures.push(
    "src/features/rules/rule-editor.tsx: Rust 异步草稿必须统一使用保存门禁和代次隔离，解析结果必须函数式合并到最新动作",
  );
}

const breakpointSource = readFileSync(
  join(sourceRoot, "features/breakpoints/breakpoints-view.tsx"),
  "utf8",
);
if (/decisionRequires(?:JsonFormatting|Validation)/.test(breakpointSource)) {
  failures.push(
    "src/features/breakpoints/breakpoints-view.tsx: 断点决策预处理策略必须由 Rust 提供",
  );
}
if (
  !breakpointSource.includes("available_actions") ||
  /<ListBox\.Item\s+id=["'](?:forward_|mock_response|delay|disconnect_|custom_http_status|invalid_json|wrong_content_length|truncate|drop_response)/.test(
    breakpointSource,
  )
) {
  failures.push(
    "src/features/breakpoints/breakpoints-view.tsx: 断点可执行动作和默认参数必须由 Rust ViewModel 提供",
  );
}

const faultSource = readFileSync(
  join(sourceRoot, "features/faults/faults-view.tsx"),
  "utf8",
);
if (/\.name\.includes\(/.test(faultSource)) {
  failures.push(
    "src/features/faults/faults-view.tsx: 禁止根据中文名称推断故障业务语义",
  );
}
if (
  /useState\(\s*(?:1|100|false)\s*\)/.test(faultSource) ||
  /channel:\s*["']transaction["']/.test(faultSource)
) {
  failures.push(
    "src/features/faults/faults-view.tsx: 故障通道、命中次数、优先级和一次性默认值必须来自 Rust 模板",
  );
}

const settingsSource = readFileSync(
  join(sourceRoot, "features/settings/settings-view.tsx"),
  "utf8",
);
if (
  /\.split\(\s*\/\[,，\]\//.test(settingsSource) ||
  !settingsSource.includes("leafSansRaw")
) {
  failures.push(
    "src/features/settings/settings-view.tsx: SAN 原始文本必须交由 Rust 原子规范化",
  );
}

const captureSourceForResume = readFileSync(
  join(sourceRoot, "features/capture/capture-view.tsx"),
  "utf8",
);
if (/after_event_id:\s*pauseCursor/.test(captureSourceForResume)) {
  failures.push(
    "src/features/capture/capture-view.tsx: 恢复滚动必须请求完整 Rust 显示快照，不能永久切换到增量游标",
  );
}

const bootstrapSource = readFileSync(
  join(sourceRoot, "features/shell/bootstrap-context.tsx"),
  "utf8",
);
if (!bootstrapSource.includes("refreshGeneration")) {
  failures.push(
    "src/features/shell/bootstrap-context.tsx: Bootstrap 刷新必须使用代次隔离迟到响应",
  );
}

if (failures.length > 0) {
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exitCode = 1;
} else {
  process.stdout.write("Frontend Rust-only boundary scan passed.\n");
}
