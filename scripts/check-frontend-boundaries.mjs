import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const sourceRoot = join(root, "src");

// 这是一个轻量级架构门禁，不是 TypeScript linter 的替代品。
// 它把项目最重要的约束变成 CI 可执行规则：前端只展示 Rust ViewModel、收集用户意图，
// 不得自行联网、持久化业务数据、解析编码或硬编码 Payment 产品语义。
function sourceFiles(directory) {
  return readdirSync(directory)
    .flatMap((name) => {
      const path = join(directory, name);
      return statSync(path).isDirectory() ? sourceFiles(path) : [path];
    })
    .filter((path) => /\.(ts|tsx)$/.test(path))
    // generated/rust-types.ts 由 Rust/Specta 生成，内容应由 bindings 差异检查负责；
    // 在这里扫描会把合法的产品 DTO 字面量误判为前端手写业务逻辑。
    .filter((path) => !path.endsWith("generated/rust-types.ts"));
}

// 页面按职责拆分后，一个功能契约可能由 facade、列表面板和详情面板共同消费。
// 架构门禁应检查整个 feature，而不是迫使所有契约重新堆回单个超长组件。
function featureModuleSource(featureName) {
  const directory = join(sourceRoot, "features", featureName);
  return readdirSync(directory)
    .filter(
      (name) =>
        /\.(?:ts|tsx)$/.test(name) &&
        !/\.(?:test|spec)\.(?:ts|tsx)$/.test(name),
    )
    .map((name) => readFileSync(join(directory, name), "utf8"))
    .join("\n");
}

// 第一层是跨文件通用禁令。正则只识别明确的危险模式，命中后要求开发者把逻辑下沉到 Rust
// 或改用 HeroUI；不要为了“绕过正则”改写同一段前端业务逻辑。
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

const themeStorageFiles = new Set([
  "src/features/theme/theme-provider.tsx",
  "src/features/theme/theme-provider.test.tsx",
]);
const iconCloseTriggerFiles = new Set([
  "src/features/capture/capture-detail-panel.tsx",
]);

const failures = [];
for (const file of sourceFiles(sourceRoot)) {
  const content = readFileSync(file, "utf8");
  const relativePath = relative(root, file);
  const isTestFile = /\.(?:test|spec)\.(?:ts|tsx)$/.test(file);
  for (const [pattern, message] of forbidden) {
    // 架构测试需要读取源码来验证 HeroUI 组合方式；Node.js API 禁令只约束会进入
    // WebView 的生产展示代码，不能反过来禁止测试工具检查这些代码。
    if (isTestFile && message === "展示层不得引入 Node.js 系统 API") continue;
    // 主题偏好不是业务数据；只允许主题状态模块及其契约测试触碰 localStorage。
    if (
      themeStorageFiles.has(relativePath) &&
      message === "前端不得持久化业务数据"
    ) {
      continue;
    }
    if (
      iconCloseTriggerFiles.has(relativePath) &&
      message.startsWith("CloseTrigger 仅用于右上角关闭图标")
    ) {
      continue;
    }
    if (pattern.test(content)) {
      failures.push(`${relativePath}: ${message}`);
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
  if (
    !isTestFile &&
    /GMO-FG|Payment|\b(?:transaction|dll|DLL)\b|16_627|16_127|16627|16127/.test(
      content,
    )
  ) {
    failures.push(
      `${relative(root, file)}: 产品名称、通道 ID、端口和专属文案必须来自 Rust DTO，禁止写死在生产前端`,
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

const captureDetailSource = readFileSync(
  join(sourceRoot, "features", "capture", "capture-detail-panel.tsx"),
  "utf8",
);
if (
  !/<Modal\.CloseTrigger(?=[^>]*aria-label="关闭详情并释放报文")[^>]*>\s*<Xmark[^>]*\/>\s*<\/Modal\.CloseTrigger>/.test(
    captureDetailSource,
  )
) {
  failures.push(
    "src/features/capture/capture-detail-panel.tsx: 抓包详情必须使用右上角纯图标 CloseTrigger",
  );
}

const themeProviderSource = readFileSync(
  join(sourceRoot, "features", "theme", "theme-provider.tsx"),
  "utf8",
);
if (
  !themeProviderSource.includes(
    'export const THEME_STORAGE_KEY = "intercept-proxy-theme";',
  ) ||
  [...themeProviderSource.matchAll(/localStorage\.(?:getItem|setItem|removeItem)\(([^,)]*)/g)].some(
    ([, keyExpression]) => keyExpression.trim() !== "THEME_STORAGE_KEY",
  )
) {
  failures.push(
    "src/features/theme/theme-provider.tsx: localStorage 只能使用固定主题偏好 key",
  );
}

// 第二层检查具体页面与 Rust 契约的对应关系。这些规则比通用正则更精确，用于防止以后重构
// 页面时把规则默认值、断点动作、通道目录或 HTTP 报文解释偷偷搬回 TypeScript。
const captureSource = featureModuleSource("capture");
if (/commands\.ruleSave/.test(captureSource)) {
  failures.push(
    "src/features/capture/capture-view.tsx: CAPTURE-009 禁止在跳转前保存规则",
  );
}

// 规则编辑器按职责拆分为 facade、模型、条件与动作模块。架构门禁必须检查整个模块组，
// 不能把“单文件包含所有 Rust Command”误当成边界要求，否则会反向鼓励超长组件。
const ruleEditorDirectory = join(sourceRoot, "features/rules");
const ruleEditorSource = readdirSync(ruleEditorDirectory)
  .filter((name) =>
    /^(?:rule-editor(?:-controls|-model)?|condition-editor|actions-editor|action-fields|terminal-action-fields)\.tsx?$/.test(
      name,
    ),
  )
  .map((name) => readFileSync(join(ruleEditorDirectory, name), "utf8"))
  .join("\n");
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

const breakpointSource = featureModuleSource("breakpoints");
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

const faultSource = featureModuleSource("faults");
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
if (
  /templates\.data\?\.[\s\S]{0,120}default_channel/.test(faultSource) ||
  !faultSource.includes("channel_catalog")
) {
  failures.push(
    "src/features/faults/faults-view.tsx: 故障可选通道必须来自完整 Rust channel catalog，不能从模板默认通道反推",
  );
}

const productChannelUiContracts = [
  ["capture", "features/capture/capture-view.tsx", ["channel_catalog", "channel_text"]],
  ["sessions", "features/sessions/sessions-view.tsx", ["channel_catalog", "channel_text"]],
  ["breakpoints", "features/breakpoints/breakpoints-view.tsx", ["channel_text"]],
  ["rules", "features/rules/rules-view.tsx", ["channel_catalog", "channel_text"]],
  ["faults", "features/faults/faults-view.tsx", ["channel_catalog"]],
];
for (const [featureName, relativePath, requiredContracts] of productChannelUiContracts) {
  const source = featureModuleSource(featureName);
  if (/["'](?:transaction|dll)["']/.test(source)) {
    failures.push(
      `src/${relativePath}: 产品通道 ID 和展示名称必须来自 Rust DTO/catalog，禁止写死 transaction/dll`,
    );
  }
  for (const contract of requiredContracts) {
    if (!source.includes(contract)) {
      failures.push(
        `src/${relativePath}: 缺少 Rust 产品通道展示契约 ${contract}`,
      );
    }
  }
}

const httpInspectionUiContracts = [
  [
    "capture",
    "features/capture/capture-view.tsx",
    ["http_status", ".headers", "HTTP 状态码"],
  ],
  [
    "sessions",
    "features/sessions/sessions-view.tsx",
    ["http_status", ".headers", "HTTP 状态码"],
  ],
];
for (const [featureName, relativePath, requiredContracts] of httpInspectionUiContracts) {
  const source = featureModuleSource(featureName);
  for (const contract of requiredContracts) {
    if (!source.includes(contract)) {
      failures.push(
        `src/${relativePath}: 缺少 Rust HTTP 报文检查契约 ${contract}`,
      );
    }
  }
}

const settingsSource = readFileSync(
  join(sourceRoot, "features/settings/settings-view.tsx"),
  "utf8",
);
if (
  /leaf_sans\s*\.\s*(?:split|join)\s*\(|leafSansRaw/.test(settingsSource) ||
  !settingsSource.includes("commands.settingsValidate(candidate)")
) {
  failures.push(
    "src/features/settings/settings-view.tsx: 系统设置必须把完整 Draft 直接交给 Rust，禁止在前端拼装或解析 SAN",
  );
}

const captureSourceForResume = captureSource;
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
  // 一次输出全部问题，便于新手按文件逐项修复，而不是每修一个再重新跑 CI 才看到下一个。
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exitCode = 1;
} else {
  process.stdout.write("Frontend Rust-only boundary scan passed.\n");
}
