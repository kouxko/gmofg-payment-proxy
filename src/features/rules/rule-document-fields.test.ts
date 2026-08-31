import { describe, expect, it } from "vitest";
import type {
  RuleDocumentActionCapability,
  RuleDocumentSchemaFieldCapability,
  RuleLocalDocumentTypeCapability,
} from "@/generated/rust-types";
import { documentEditorFields } from "./rule-document-fields";
import { documentSchemaFields } from "./rule-document-schema";

const nodeAction = (
  kind: "set" | "clear",
  valueType: "string" | "array",
): RuleDocumentActionCapability => ({
  kind,
  target_kind: "node",
  target_value_type: valueType,
  operand_value_type: kind === "set" ? valueType : null,
});

const arrayAction = (
  kind: "insert" | "append",
  operandValueType: "string" | "array",
): RuleDocumentActionCapability => ({
  kind,
  target_kind: "array",
  target_value_type: "array",
  operand_value_type: operandValueType,
});

const schemaFreeString: RuleLocalDocumentTypeCapability = {
  value_type: "string",
  predicates: ["equals"],
  actions: [
    nodeAction("set", "string"),
    nodeAction("clear", "string"),
    arrayAction("insert", "string"),
    arrayAction("append", "string"),
  ],
};

describe("Rust-authored Document schema field capabilities", () => {
  it("does not enrich a scalar schema field with schema-free Insert or Append", () => {
    const capabilities: RuleDocumentSchemaFieldCapability[] = [{
      path: "/name",
      label: "Name",
      value_type: "string",
      item_template: false,
      predicates: ["equals", "contains", "starts_with", "ends_with"],
      actions: [nodeAction("set", "string"), nodeAction("clear", "string")],
    }];

    const fields = documentEditorFields(
      documentSchemaFields(capabilities),
      [],
      [],
      [schemaFreeString],
    );

    expect(fields[0]?.actions.map((action) => action.kind)).toEqual(["set", "clear"]);
  });

  it("uses string operands for Insert and Append on array<string>", () => {
    const capabilities: RuleDocumentSchemaFieldCapability[] = [{
      path: "/tags",
      label: "Tags",
      value_type: "array",
      item_template: false,
      predicates: [],
      actions: [
        nodeAction("set", "array"),
        nodeAction("clear", "array"),
        arrayAction("insert", "string"),
        arrayAction("append", "string"),
      ],
    }];

    const actions = documentSchemaFields(capabilities)[0]?.actions ?? [];
    expect(actions.filter((action) => action.target_kind === "array")).toEqual([
      arrayAction("insert", "string"),
      arrayAction("append", "string"),
    ]);
  });

  it("keeps a nested array as the outer array operand type", () => {
    const capabilities: RuleDocumentSchemaFieldCapability[] = [
      {
        path: "/matrix",
        label: "Matrix",
        value_type: "array",
        item_template: false,
        predicates: [],
        actions: [arrayAction("append", "array")],
      },
      {
        path: "/matrix/*",
        label: "/matrix/*",
        value_type: "array",
        item_template: true,
        predicates: [],
        actions: [],
      },
    ];

    const fields = documentSchemaFields(capabilities);
    expect(fields[0]?.actions[0]?.operand_value_type).toBe("array");
    expect(fields[1]?.actions).toEqual([]);
  });

  it("restores a schema-free Insert field with its explicit array target and string operand descriptors", () => {
    const fields = documentEditorFields(
      [],
      [],
      [{ source: "document", value: { type: "insert", path: "/local-items", index: 0, value: "item" } }],
      [schemaFreeString],
    );

    expect(fields).toHaveLength(1);
    expect(fields[0]?.name).toBe("/local-items");
    expect(fields[0]?.actions).toContainEqual(arrayAction("insert", "string"));
    expect(fields[0]?.actions).toContainEqual(arrayAction("append", "string"));
  });
});
