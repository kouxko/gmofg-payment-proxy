import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const tauriConfig = JSON.parse(
  readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"),
);
const forbidden = [
  /GMO-FG/i,
  /gmofg/i,
  /Payment Proxy/i,
  /\bPayment\b/i,
  /\bDLL\b/i,
  /\bD48\b/i,
  /https\.gmo-fg\.net/i,
  /\b16(?:127|627)\b/,
];
const compiledForbidden = [
  /GMO-FG/i,
  /gmofg/i,
  // D48 是测试报告中的大写业务码。二进制指令/压缩字节可能形成类似 `d48*`
  // 的短小写 ASCII 片段；对编译产物仅拦截准确的大写业务码，文本资源仍保持
  // 不区分大小写的严格扫描。
  /\bD48\b/,
  /https\.gmo-fg\.net/i,
];
const textExtensions = new Set([
  ".css",
  ".html",
  ".js",
  ".json",
  ".kt",
  ".kts",
  ".md",
  ".plist",
  ".toml",
  ".ts",
  ".tsx",
  ".xml",
]);

function files(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

// 只扫描真正进入桌面包的静态前端和 Tauri 配置。测试代码、兼容报告和手工 Workspace
// 可以描述具体被测系统，但这些内容不得被打进 Intercept Proxy 默认安装包。
const candidates = [
  ...files(join(root, "out")),
  ...files(join(root, "android-companion", "app", "src", "main")),
  ...files(join(root, "src-tauri", "resources")),
  join(root, "src-tauri", "tauri.conf.json"),
  ...files(join(root, "src-tauri", "target", "release", "bundle")).filter(
    (path) => {
      const relativePath = relative(root, path).toLocaleLowerCase("en-US");
      // DMG 是压缩磁盘镜像；直接扫描压缩字节会把随机的短 ASCII 片段误判成
      // 业务标识。镜像内实际交付的 .app 已在同一 bundle 目录逐文件扫描，因此
      // 排除容器本身不会降低门禁覆盖，反而避免不可复现的压缩噪声。
      return (
        !relativePath.endsWith(".dmg") &&
        relativePath.includes(tauriConfig.productName.toLocaleLowerCase("en-US"))
      );
    },
  ),
].filter((path) => existsSync(path));

const failures = [];
for (const path of candidates) {
  const bytes = readFileSync(path);
  const isText = textExtensions.has(extname(path).toLocaleLowerCase("en-US"));
  // 文本资源严格禁止全部业务词。编译产物只扫描可打印字符串中的强业务标识：HTTP
  // 依赖天然包含 “Payment Required”，ZIP/ELF 压缩字节也可能随机出现三字符 DLL，
  // 直接对任意二进制字节做全文正则会产生无法消除的误报。
  const text = isText ? bytes.toString("utf8") : printableAsciiStrings(bytes, 4);
  const patterns = isText ? forbidden : compiledForbidden;
  for (const pattern of patterns) {
    if (pattern.test(text)) {
      failures.push(`${relative(root, path)} 命中 ${pattern}`);
    }
  }
}

function printableAsciiStrings(bytes, minimumLength) {
  const strings = [];
  let start = 0;
  for (let index = 0; index <= bytes.length; index += 1) {
    const byte = bytes[index];
    const printable = index < bytes.length && byte >= 0x20 && byte <= 0x7e;
    if (printable) continue;
    if (index - start >= minimumLength) {
      strings.push(bytes.subarray(start, index).toString("ascii"));
    }
    start = index + 1;
  }
  return strings.join("\n");
}

for (const resource of tauriConfig.bundle?.resources ?? []) {
  if (/server\.crt|\.p12|\.pfx|gmofg|payment/i.test(String(resource))) {
    failures.push(`tauri.conf.json 禁止打包业务证书或业务资源：${resource}`);
  }
}

if (failures.length > 0) {
  console.error("Intercept Proxy 安装包品牌/资源门禁失败：");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log(`安装包品牌/资源门禁通过，共扫描 ${candidates.length} 个文件。`);
