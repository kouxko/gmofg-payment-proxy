import assert from "node:assert/strict";
import test from "node:test";

import { findPhase3LegacyContracts } from "./check-task-20260829-002-phase3-contract.mjs";

test("accepts recursive Document contracts without field-slot remnants", () => {
  assert.deepEqual(
    findPhase3LegacyContracts({
      "domain.rs": "enum DocumentAction { RecordMatch, SetField, ClearField }",
      "rust-types.ts": "type DocumentValue = string | number | boolean | null",
    }),
    [],
  );
});

for (const contract of [
  "ClearDocument",
  "clear_document",
  "字段值槽",
  "Schema 身份和结构",
  "MAX_PROTOCOL_RULE_INT_TEXT_BYTES",
  "Blob Hex",
  "整数文本不能超过",
]) {
  test(`rejects legacy contract ${contract}`, () => {
    assert.deepEqual(findPhase3LegacyContracts({ "source.rs": contract }), [
      `source.rs: forbidden ${contract}`,
    ]);
  });
}
