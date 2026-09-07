import assert from "node:assert/strict";
import test from "node:test";

import { validateDecisionPolicy } from "./check-architecture-docs.mjs";

const predecessor = "decisions/ADR-004-old.md";
const successor = "decisions/ADR-008-new.md";

function contracts() {
  return new Map([
    [
      predecessor,
      {
        status: "Superseded by [ADR-008](ADR-008-new.md)",
        supersededBy: successor,
      },
    ],
    [
      successor,
      {
        status: "Accepted",
        supersedes: predecessor,
      },
    ],
  ]);
}

function sources({
  predecessorStatus = "Superseded by [ADR-008](ADR-008-new.md)",
  reverseLink = "- Supersedes: [ADR-004](ADR-004-old.md)",
  alternatives = "## Alternatives\n\n- Rejected: retain the old decision.\n",
} = {}) {
  return new Map([
    [predecessor, `# Old\n\n- Status: ${predecessorStatus}\n`],
    [
      successor,
      `# New\n\n- Status: Accepted\n${reverseLink}\n\n${alternatives}`,
    ],
  ]);
}

test("accepts an exact supersession chain with rejected alternatives", () => {
  assert.deepEqual(validateDecisionPolicy(sources(), contracts()), []);
});

test("rejects an arbitrary Superseded status", () => {
  assert.match(
    validateDecisionPolicy(
      sources({ predecessorStatus: "Superseded" }),
      contracts(),
    ).join("\n"),
    /expected exact Status/u,
  );
});

test("rejects a status that names the wrong successor", () => {
  assert.match(
    validateDecisionPolicy(
      sources({
        predecessorStatus: "Superseded by [ADR-009](ADR-009-other.md)",
      }),
      contracts(),
    ).join("\n"),
    /expected exact Status/u,
  );
});

test("rejects a missing reverse Supersedes link", () => {
  assert.match(
    validateDecisionPolicy(sources({ reverseLink: "" }), contracts()).join(
      "\n",
    ),
    /missing exact reverse link/u,
  );
});

test("accepted decisions still require explicit rejected alternatives", () => {
  assert.match(
    validateDecisionPolicy(sources({ alternatives: "" }), contracts()).join(
      "\n",
    ),
    /rejected alternatives are not explicit/u,
  );
});

test("accepts one decision that supersedes multiple bidirectional predecessors", () => {
  const first = "decisions/ADR-002-first.md";
  const second = "decisions/ADR-007-second.md";
  const next = "decisions/ADR-009-next.md";
  const decisionContracts = new Map([
    [first, { status: "Superseded", supersededBy: next }],
    [second, { status: "Superseded", supersededBy: next }],
    [next, { status: "Accepted", supersedes: [first, second] }],
  ]);
  const decisionSources = new Map([
    [first, "# First\n\n- Status: Superseded\n"],
    [second, "# Second\n\n- Status: Superseded\n"],
    [next, "# Next\n\n- Status: Accepted\n- Supersedes: [ADR-002](ADR-002-first.md)、[ADR-007](ADR-007-second.md)\n\n## Alternatives\n\n- Rejected: keep both predecessors active.\n"],
  ]);
  assert.deepEqual(validateDecisionPolicy(decisionSources, decisionContracts), []);
});

test("rejects a missing link from a multi-predecessor decision", () => {
  const first = "decisions/ADR-002-first.md";
  const second = "decisions/ADR-007-second.md";
  const next = "decisions/ADR-009-next.md";
  const failures = validateDecisionPolicy(
    new Map([
      [first, "# First\n\n- Status: Superseded\n"],
      [second, "# Second\n\n- Status: Superseded\n"],
      [next, "# Next\n\n- Status: Accepted\n- Supersedes: [ADR-002](ADR-002-first.md)\n\n## Alternatives\n\n- Rejected: keep both predecessors active.\n"],
    ]),
    new Map([
      [first, { status: "Superseded", supersededBy: next }],
      [second, { status: "Superseded", supersededBy: next }],
      [next, { status: "Accepted", supersedes: [first, second] }],
    ]),
  );
  assert.match(failures.join("\n"), /missing reverse link \[ADR-007\]/u);
});
