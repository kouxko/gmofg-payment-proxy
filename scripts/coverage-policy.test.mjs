import assert from "node:assert/strict";
import test from "node:test";

import { assertCoverage, evaluateCoverage } from "./coverage-policy.mjs";

test("accepts a report that meets every configured threshold", () => {
  assert.deepEqual(
    evaluateCoverage(
      "fixture",
      { functions: 90, lines: 91.25, regions: 92 },
      { functions: 90, lines: 90, regions: 90 },
    ),
    [],
  );
});

test("reports every metric below its threshold instead of stopping at the first", () => {
  assert.deepEqual(
    evaluateCoverage(
      "fixture",
      { functions: 89.99, lines: 70, regions: 80 },
      { functions: 90, lines: 90, regions: 90 },
    ),
    [
      "fixture: functions coverage 89.99% is below 90.00%",
      "fixture: lines coverage 70.00% is below 90.00%",
      "fixture: regions coverage 80.00% is below 90.00%",
    ],
  );
});

test("fails closed for missing metrics, unknown metrics and invalid thresholds", () => {
  assert.deepEqual(
    evaluateCoverage(
      "fixture",
      { lines: 100 },
      { functions: 90, lines: 101, mystery: 90 },
    ),
    [
      "fixture: report is missing functions coverage",
      "fixture: invalid lines threshold 101",
      "fixture: unknown coverage metric mystery",
    ],
  );
});

test("assertCoverage throws for a failing fixture and returns for a passing fixture", () => {
  assert.throws(
    () => assertCoverage("fixture", { lines: 89 }, { lines: 90 }),
    /lines coverage 89\.00% is below 90\.00%/,
  );
  assert.doesNotThrow(() => assertCoverage("fixture", { lines: 90 }, { lines: 90 }));
});
