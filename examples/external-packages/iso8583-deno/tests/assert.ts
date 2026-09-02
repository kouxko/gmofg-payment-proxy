export function assert(condition: unknown, message = "assertion failed"): asserts condition {
  if (!condition) throw new Error(message);
}

export function assertEquals<T>(actual: T, expected: T): void {
  const left = JSON.stringify(actual);
  const right = JSON.stringify(expected);
  if (left !== right) throw new Error(`expected ${right}, received ${left}`);
}

export function assertThrows(action: () => unknown, includes: string): void {
  try {
    action();
  } catch (error) {
    assert(error instanceof Error, "expected Error");
    assert(error.message.includes(includes), `expected error containing ${includes}`);
    return;
  }
  throw new Error("expected action to throw");
}
