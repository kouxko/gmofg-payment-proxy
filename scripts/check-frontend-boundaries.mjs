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

// 测试夹具与测试套件都不会进入 WebView bundle。夹具使用明确的 `.test-support`
// 后缀，避免伪装成会被 Vitest 收集的 `.test` 文件或为单个文件增加绕过白名单。
function isTestArtifact(path) {
  return /\.(?:test|spec|test-support)\.(?:ts|tsx)$/.test(path);
}

// 页面按职责拆分后，一个功能契约可能由 facade、列表面板和详情面板共同消费。
// 架构门禁应检查整个 feature，而不是迫使所有契约重新堆回单个超长组件。
function featureModuleSource(featureName) {
  const directory = join(sourceRoot, "features", featureName);
  return readdirSync(directory)
    .filter(
      (name) =>
        /\.(?:ts|tsx)$/.test(name) &&
        !isTestArtifact(name),
    )
    .map((name) => readFileSync(join(directory, name), "utf8"))
    .join("\n");
}

function protocolRuleBoundaryCodes(source) {
  const codes = [];
  if (!source.includes("commands.ruleEditorContext")) {
    codes.push("PROTOCOL_RULE_EDITOR_CONTEXT_MISSING");
  }
  if (/\b(?:listenerStages|newProtocolRuleDraft)\b|commands\.protocolRuleCapabilities/.test(source)) {
    codes.push("PROTOCOL_RULE_LEGACY_BUSINESS_HELPER");
  }
  const definitions = new Map();
  for (const match of source.matchAll(
    /\b(?:const|let)\s+([A-Za-z_$][A-Za-z0-9_$]*)(?:\s*:\s*[^=;\n]+)?\s*=\s*(\[[^\]]*\])/g,
  )) {
    definitions.set(match[1], match[2]);
  }
  const ownershipStatements = [...new Set([
    ...source.split(";"),
    ...source.split(/\r?\n/),
  ].filter((statement) => /\btopology\b/.test(statement)))];
  for (const statement of ownershipStatements) {
    const referencedDefinitions = [...definitions]
      .filter(([identifier]) => new RegExp(`\\b${identifier}\\b`).test(statement))
      .map(([, definition]) => definition)
      .join(" ");
    const ownedSource = `${statement} ${referencedDefinitions}`;
    const stages = ownedSource.match(
      /["'](?:app_to_proxy|proxy_to_upstream|upstream_to_proxy|proxy_to_app)["']/g,
    ) ?? [];
    if (new Set(stages).size >= 2) {
      codes.push("PROTOCOL_RULE_TOPOLOGY_MATRIX");
    }
    if (/["'](?:record_match|clear_document|set_field|clear_field)["']/.test(ownedSource)) {
      codes.push("PROTOCOL_RULE_DEFAULT_ACTION_MATRIX");
    }
  }
  return [...new Set(codes)].sort();
}

function generatedProtocolRuleBindingCodes(source) {
  const codes = [];
  const hasTypedSignature = /ruleEditorContext:\s*\(listenerId:\s*ListenerId\)/.test(source);
  const hasCamelCaseInvoke = /__TAURI_INVOKE\("rule_editor_context",\s*\{\s*listenerId\s*\}\)/.test(source);
  if (!hasTypedSignature || !hasCamelCaseInvoke) {
    codes.push("PROTOCOL_RULE_GENERATED_IPC_MISSING");
  }
  if (!source.includes("export type RuleEditorContext = {")
    || !source.includes("new_rule_draft: RuleNewDefinitionDraft")) {
    codes.push("PROTOCOL_RULE_EDITOR_DTO_MISSING");
  }
  return codes;
}

function tauriProtocolRuleRegistrationCodes(source) {
  return source.includes("rule_editor_context,")
    ? []
    : ["PROTOCOL_RULE_TAURI_REGISTRATION_MISSING"];
}

const protocolRuleBoundaryFixtures = [
  [
    "Rust editor context is the only topology source",
    "const context = commands.ruleEditorContext(listenerId); const stages = context.stages;",
    [],
  ],
  [
    "frontend listener topology stage matrix is rejected",
    "const relayOrLocalChoices = listener.data_plane.settings.topology.mode === 'local_responder' ? ['app_to_proxy'] : ['app_to_proxy', 'proxy_to_upstream']; commands.ruleEditorContext(listener.id);",
    ["PROTOCOL_RULE_TOPOLOGY_MATRIX"],
  ],
  [
    "frontend topology-specific default action matrix is rejected",
    "const initialBehavior = listener.topology.mode === 'local_responder' ? [{ type: 'record_match' }] : [{ type: 'clear_document' }]; commands.ruleEditorContext(listener.id);",
    ["PROTOCOL_RULE_DEFAULT_ACTION_MATRIX"],
  ],
  [
    "typed const topology stage matrix is rejected",
    "const choices: RuleStage[] = listener.topology.mode === 'local_responder' ? ['app_to_proxy'] : ['app_to_proxy', 'proxy_to_upstream']; commands.ruleEditorContext(listener.id);",
    ["PROTOCOL_RULE_TOPOLOGY_MATRIX"],
  ],
  [
    "return ternary default action matrix is rejected",
    "commands.ruleEditorContext(listener.id); function draft(listener: Listener) { return listener.topology.mode === 'local_responder' ? [{ type: 'record_match' }] : [{ type: 'clear_document' }] }",
    ["PROTOCOL_RULE_DEFAULT_ACTION_MATRIX"],
  ],
  [
    "ASI topology stage matrix is rejected",
    "commands.ruleEditorContext(listener.id)\nconst choices = listener.topology.mode === 'local_responder' ? ['app_to_proxy'] : ['app_to_proxy', 'proxy_to_upstream']",
    ["PROTOCOL_RULE_TOPOLOGY_MATRIX"],
  ],
  [
    "indirect topology stage arrays are rejected",
    "const local = ['app_to_proxy']; const relay = ['app_to_proxy', 'proxy_to_upstream']; const choices = listener.topology.mode === 'local_responder' ? local : relay; commands.ruleEditorContext(listener.id);",
    ["PROTOCOL_RULE_TOPOLOGY_MATRIX"],
  ],
  [
    "legacy per-stage capability command is rejected",
    "commands.ruleEditorContext(listener.id); commands.protocolRuleCapabilities(listener.id, stage);",
    ["PROTOCOL_RULE_LEGACY_BUSINESS_HELPER"],
  ],
  [
    "missing Rust editor context is rejected",
    "const stages = server.stages;",
    ["PROTOCOL_RULE_EDITOR_CONTEXT_MISSING"],
  ],
];

const generatedProtocolRuleBindingFixtures = [
  [
    "generated binding keeps camelCase argument translation and editor DTO",
    'ruleEditorContext: (listenerId: ListenerId) => __TAURI_INVOKE("rule_editor_context", { listenerId }); export type RuleEditorContext = { listener_id: ListenerId; stages: RuleEditorStage[] }; export type HttpRuleEditorStage = { new_rule_draft: RuleNewDefinitionDraft };',
    [],
  ],
  [
    "generated binding cannot regress to a snake_case caller argument",
    'ruleEditorContext: (listener_id: ListenerId) => __TAURI_INVOKE("rule_editor_context", { listener_id }); export type RuleEditorContext = { listener_id: ListenerId; stages: RuleEditorStage[] }; export type HttpRuleEditorStage = { new_rule_draft: RuleNewDefinitionDraft };',
    ["PROTOCOL_RULE_GENERATED_IPC_MISSING"],
  ],
  [
    "generated editor context rejects the old save-input draft DTO",
    'ruleEditorContext: (listenerId: ListenerId) => __TAURI_INVOKE("rule_editor_context", { listenerId }); export type RuleEditorContext = { listener_id: ListenerId; stages: RuleEditorStage[] }; export type HttpRuleEditorStage = { new_rule_draft: RuleDefinitionSaveInput };',
    ["PROTOCOL_RULE_EDITOR_DTO_MISSING"],
  ],
  [
    "generated editor context must retain the Rust draft DTO",
    'ruleEditorContext: (listenerId: ListenerId) => __TAURI_INVOKE("rule_editor_context", { listenerId });',
    ["PROTOCOL_RULE_EDITOR_DTO_MISSING"],
  ],
];

for (const [name, source, expected] of protocolRuleBoundaryFixtures) {
  const actual = protocolRuleBoundaryCodes(source).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) {
    process.stderr.write(
      `Frontend boundary fixture ${name}: expected [${expected}], got [${actual}]\n`,
    );
    process.exitCode = 1;
  }
}
for (const [name, source, expected] of generatedProtocolRuleBindingFixtures) {
  const actual = generatedProtocolRuleBindingCodes(source).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) {
    process.stderr.write(
      `Frontend generated-binding fixture ${name}: expected [${expected}], got [${actual}]\n`,
    );
    process.exitCode = 1;
  }
}
for (const [name, source, expected] of [
  ["Tauri command is registered", "rule_editor_context,", []],
  ["missing Tauri command registration is rejected", "protocol_rule_list,", ["PROTOCOL_RULE_TAURI_REGISTRATION_MISSING"]],
]) {
  const actual = tauriProtocolRuleRegistrationCodes(source);
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    process.stderr.write(
      `Frontend Tauri-registration fixture ${name}: expected [${expected}], got [${actual}]\n`,
    );
    process.exitCode = 1;
  }
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
  "src/features/capture/exchange-observation-detail.tsx",
  "src/features/listeners/socket-protocol-package-dialog.tsx",
  "src/features/protocol-packages/protocol-package-dialog.tsx",
  "src/features/rules/rules-view.tsx",
  "src/features/rules/rule-creation-dialogs.tsx",
]);

const failures = [];
for (const file of sourceFiles(sourceRoot)) {
  const content = readFileSync(file, "utf8");
  const relativePath = relative(root, file);
  const isTestFile = isTestArtifact(file);
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

for (const [path, label] of [
  ["src/features/capture/exchange-observation-detail.tsx", "关闭 Exchange 详情"],
]) {
  const source = readFileSync(join(root, path), "utf8");
  const contract = new RegExp(
    `<Modal\\.CloseTrigger(?=[^>]*aria-label="${label}")[^>]*>\\s*<Xmark[^>]*\\/>\\s*</Modal\\.CloseTrigger>`,
  );
  if (!contract.test(source)) {
    failures.push(`${path}: 详情弹窗必须使用右上角纯图标 CloseTrigger`);
  }
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
    /^(?:rule-definition-editor|use-async-request-slots)\.tsx?$/.test(
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
  "commands.ruleDefinitionNthHitConditionDraft",
  "commands.ruleDefinitionHttpConditionDraft",
  "commands.ruleDefinitionActionDraft",
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
  !ruleEditorSource.includes("editorScope") ||
  !ruleEditorSource.includes("appendHttpFactoryResult") ||
  !ruleEditorSource.includes("generations.current.get(key) !== generation")
) {
  failures.push(
    "src/features/rules/rule-definition-editor.tsx: Rust 异步草稿必须按规则、Listener、阶段使用代次隔离，并函数式合并到最新草稿",
  );
}

const protocolRuleSource = featureModuleSource("rules");
const protocolRuleBoundaryFailures = protocolRuleBoundaryCodes(protocolRuleSource);
if (protocolRuleBoundaryFailures.includes("PROTOCOL_RULE_EDITOR_CONTEXT_MISSING")) {
  failures.push(
    "src/features/rules: 统一规则编辑器必须从 Rust ruleEditorContext 取得全部阶段、能力和新规则草稿",
  );
}
if (protocolRuleBoundaryFailures.some((code) => code !== "PROTOCOL_RULE_EDITOR_CONTEXT_MISSING")) {
  failures.push(
    "src/features/rules: 禁止在前端推导协议规则阶段或新规则默认值，也不得按前端选择的 stage 查询旧能力接口",
  );
}

const generatedBindingsSource = readFileSync(
  join(sourceRoot, "generated", "rust-types.ts"),
  "utf8",
);
if (generatedProtocolRuleBindingCodes(generatedBindingsSource).length > 0) {
  failures.push(
    "src/generated/rust-types.ts: 缺少 rule_editor_context 的 camelCase 参数绑定或完整编辑上下文 DTO",
  );
}

const tauriCommandsSource = readFileSync(
  join(root, "src-tauri", "src", "commands", "mod.rs"),
  "utf8",
);
if (tauriProtocolRuleRegistrationCodes(tauriCommandsSource).length > 0) {
  failures.push(
    "src-tauri/src/commands/mod.rs: rule_editor_context 必须注册到 Tauri handler",
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
  /const\s*\[\s*(?:nthHit|priority|oneShot|channel)\s*,[^\]]+\]\s*=\s*useState\(\s*(?:1|100|false|["']transaction["'])\s*\)/.test(
    faultSource,
  ) ||
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
  ["breakpoints", "features/breakpoints/breakpoints-view.tsx", ["channel_text"]],
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

// Exchange observation 有独立 Rust DTO 和查询面，不能重新借用 HTTP Message 控件。
// Display HTML 永远是不可信输入：应用 DOM 不直接注入，只允许进入无能力 iframe，
// 并由 iframe 内层 deny-by-default CSP 再封一层外链和应用 API 边界。
const exchangeObservationFiles = readdirSync(join(sourceRoot, "features", "capture"))
  .filter(
    (name) =>
      /^exchange-observation-.*\.(?:ts|tsx)$/.test(name) &&
      !/\.(?:test|spec)\.(?:ts|tsx)$/.test(name),
  );
const exchangeObservationSource = exchangeObservationFiles
  .map((name) => readFileSync(join(sourceRoot, "features", "capture", name), "utf8"))
  .join("\n");
for (const contract of [
  "commands.exchangeObservationQuery",
  "commands.exchangeObservationGet",
  "commands.exchangeObservationClear(targetWorkspaceId, true)",
]) {
  if (!exchangeObservationSource.includes(contract)) {
    failures.push(
      `src/features/capture/exchange-observation-*: 缺少 Exchange observation 契约 ${contract}`,
    );
  }
}
const protocolSafeDisplayPath = "src/features/shared/protocol-safe-display.tsx";
const protocolSafeDisplaySource = readFileSync(join(root, protocolSafeDisplayPath), "utf8");
const sharedFeatureDirectory = join(sourceRoot, "features", "shared");
for (const name of readdirSync(sharedFeatureDirectory)) {
  if (!/\.(?:ts|tsx)$/.test(name) || isTestArtifact(name)) continue;
  const source = readFileSync(join(sharedFeatureDirectory, name), "utf8");
  if (/from\s+["'](?:@\/features\/(?!shared\/)|\.\.\/)/.test(source)) {
    failures.push(
      `src/features/shared/${name}: shared 组件不得反向依赖具体 feature`,
    );
  }
}
for (const contract of [
  'sandbox=""',
  'referrerPolicy="no-referrer"',
  "default-src 'none'",
  "connect-src 'none'",
  "frame-src 'none'",
]) {
  if (!protocolSafeDisplaySource.includes(contract)) {
    failures.push(
      `${protocolSafeDisplayPath}: 缺少安全 Display 契约 ${contract}`,
    );
  }
}
if (/dangerouslySetInnerHTML/.test(protocolSafeDisplaySource)) {
  failures.push(
    `${protocolSafeDisplayPath}: 不可信 Display 禁止注入应用 DOM，必须使用无能力 sandbox iframe`,
  );
}
if (/allow-(?:scripts|same-origin|forms|popups|top-navigation|downloads)/.test(protocolSafeDisplaySource)) {
  failures.push(
    `${protocolSafeDisplayPath}: Display sandbox 禁止获得脚本、同源、表单、弹窗、顶层导航或下载能力`,
  );
}
const exchangeObservationUiFiles = exchangeObservationFiles
  .filter((name) => name !== "exchange-observation-test-fixture.ts");
const exchangeObservationUiSource = exchangeObservationUiFiles
  .map((name) => readFileSync(join(sourceRoot, "features", "capture", name), "utf8"))
  .join("\n");
if (/\.headers\b|\.http_status\b|\.target\b|JSONPath|HTTP 状态码|Cookie/.test(exchangeObservationUiSource)) {
  failures.push(
    "src/features/capture/exchange-observation-*: 禁止消费 HTTP Header/Cookie/Status/Target/JSONPath",
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
