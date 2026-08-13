const METRIC_NAMES = ["statements", "branches", "functions", "lines", "regions"];

export function evaluateCoverage(label, metrics, thresholds) {
  const failures = [];

  for (const [metric, minimum] of Object.entries(thresholds)) {
    if (!METRIC_NAMES.includes(metric)) {
      failures.push(`${label}: unknown coverage metric ${metric}`);
      continue;
    }
    if (!Number.isFinite(minimum) || minimum < 0 || minimum > 100) {
      failures.push(`${label}: invalid ${metric} threshold ${String(minimum)}`);
      continue;
    }

    const actual = metrics[metric];
    if (!Number.isFinite(actual)) {
      failures.push(`${label}: report is missing ${metric} coverage`);
    } else if (actual < minimum) {
      failures.push(
        `${label}: ${metric} coverage ${actual.toFixed(2)}% is below ${minimum.toFixed(2)}%`,
      );
    }
  }

  return failures;
}

export function assertCoverage(label, metrics, thresholds) {
  const failures = evaluateCoverage(label, metrics, thresholds);
  if (failures.length > 0) {
    throw new Error(failures.join("\n"));
  }
}
