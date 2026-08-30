import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

const policy = new Map([
  ["intercept-proxy-domain", new Set()],
  ["intercept-proxy-package-contract", new Set(["intercept-proxy-domain"])],
  ["intercept-proxy-package-runtime", new Set([
    "intercept-proxy-domain",
    "intercept-proxy-package-contract",
  ])],
  ["intercept-proxy-product-api", new Set()],
  ["intercept-proxy-exchange", new Set(["intercept-proxy-domain"])],
  ["intercept-proxy-runtime", new Set(["intercept-proxy-exchange"])],
  ["intercept-proxy-protocol-scripting", new Set([
    "intercept-proxy-domain",
    "intercept-proxy-package-runtime",
  ])],
  ["intercept-proxy-application", new Set([
    "intercept-proxy-domain",
    "intercept-proxy-exchange",
    "intercept-proxy-product-api",
  ])],
  ["intercept-proxy-android-engine", new Set(["intercept-proxy-domain"])],
  ["intercept-proxy-infrastructure", new Set([
    "intercept-proxy-application",
    "intercept-proxy-domain",
    "intercept-proxy-exchange",
    "intercept-proxy-product-api",
    "intercept-proxy-package-contract",
    "intercept-proxy-package-runtime",
    "intercept-proxy-protocol-scripting",
    "intercept-proxy-runtime",
  ])],
  ["intercept-proxy-host", new Set([
    "intercept-proxy-application",
    "intercept-proxy-infrastructure",
    "intercept-proxy-product-api",
    "intercept-proxy-runtime",
  ])],
]);

function packageName(manifest) {
  let inPackage = false;
  for (const line of manifest.split(/\r?\n/u)) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/u)?.[1];
    if (section !== undefined) {
      inPackage = section === "package";
      continue;
    }
    if (!inPackage) continue;
    const name = line.match(/^\s*name\s*=\s*["']([^"']+)["']/u)?.[1];
    if (name) return name;
  }
  return undefined;
}

function dependencyNames(manifest) {
  const names = new Set();
  let dependencies = false;
  for (const line of manifest.split(/\r?\n/u)) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/u)?.[1];
    if (section !== undefined) {
      dependencies = /(?:^|\.)((?:dev-|build-)?dependencies)$/u.test(section);
      continue;
    }
    if (!dependencies || /^\s*(?:#|$)/u.test(line)) continue;
    const declaration = line.match(/^\s*([A-Za-z0-9_-]+)(?:\.[A-Za-z0-9_-]+)?\s*=\s*(.*)$/u);
    if (!declaration) continue;
    const [, key, value] = declaration;
    const alias = value.match(/\bpackage\s*=\s*["'](intercept-proxy-[^"']+)["']/u)?.[1];
    if (alias) names.add(alias);
    else if (key.startsWith("intercept-proxy-")) names.add(key);
  }
  return names;
}

function violation(code, file, message) {
  return { code, file, message };
}

export async function checkCrateDependencies(root) {
  const cratesRoot = path.join(root, "src-tauri/crates");
  const violations = [];
  let directories;
  try {
    directories = await readdir(cratesRoot, { withFileTypes: true });
  } catch {
    return violations;
  }
  for (const directory of directories.filter((entry) => entry.isDirectory())) {
    const manifestPath = path.join(cratesRoot, directory.name, "Cargo.toml");
    let manifest;
    try {
      manifest = await readFile(manifestPath, "utf8");
    } catch {
      continue;
    }
    const crate = packageName(manifest);
    if (!crate?.startsWith("intercept-proxy-")) continue;
    const allowed = policy.get(crate);
    const file = path.relative(root, manifestPath).split(path.sep).join("/");
    if (!allowed) {
      violations.push(violation("CRATE_UNKNOWN", file, `internal crate ${crate} has no dependency policy`));
      continue;
    }
    for (const dependency of dependencyNames(manifest)) {
      if (!policy.has(dependency)) violations.push(violation("CRATE_UNKNOWN_TARGET", file, `internal dependency ${dependency} has no dependency policy`));
      else if (!allowed.has(dependency)) violations.push(violation("CRATE_DIRECTION", file, `${crate} must not depend on ${dependency}`));
    }
  }
  return violations;
}
