import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const featuresRoot = join(process.cwd(), "src", "features");

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    if (!entry.name.endsWith(".tsx") || entry.name.endsWith(".test.tsx")) return [];
    return [path];
  });
}

function tagName(node: ts.JsxElement): string {
  return node.openingElement.tagName.getText();
}

function childElements(node: ts.JsxElement): ts.JsxElement[] {
  return node.children.filter(ts.isJsxElement);
}

function descendantsNamed(node: ts.Node, name: string): ts.JsxElement[] {
  const matches: ts.JsxElement[] = [];
  node.forEachChild(function visit(child) {
    if (ts.isJsxElement(child) && tagName(child) === name) matches.push(child);
    child.forEachChild(visit);
  });
  return matches;
}

describe("HeroUI Switch composition", () => {
  it("keeps every visible control inside the clickable Switch.Content", () => {
    for (const path of sourceFiles(featuresRoot)) {
      const source = ts.createSourceFile(
        path,
        readFileSync(path, "utf8"),
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.TSX,
      );

      source.forEachChild(function visit(node) {
        if (ts.isJsxElement(node) && tagName(node) === "Switch") {
          const directChildren = childElements(node);
          const contents = directChildren.filter(
            (child) => tagName(child) === "Switch.Content",
          );
          const directControls = directChildren.filter(
            (child) => tagName(child) === "Switch.Control",
          );
          const position = source.getLineAndCharacterOfPosition(node.getStart());
          const location = `${path}:${position.line + 1}`;

          expect(directControls, `${location} 的轨道不能放在可点击内容之外`).toHaveLength(0);
          expect(contents, `${location} 缺少 HeroUI Switch.Content`).toHaveLength(1);
          expect(
            descendantsNamed(contents[0], "Switch.Control"),
            `${location} 的 Switch.Content 必须包含可点击轨道`,
          ).toHaveLength(1);
        }
        node.forEachChild(visit);
      });
    }
  });
});
