function blank(character) {
  return character === "\n" || character === "\r" ? character : " ";
}

function maskRange(output, source, start, end) {
  for (let index = start; index < end; index += 1) output[index] = blank(source[index]);
}

function rawStringEnd(source, start) {
  const prefix = source.slice(start).match(/^(?:br|r)(#+)?"/u);
  if (!prefix) return undefined;
  const hashes = prefix[1] ?? "";
  const close = `"${hashes}`;
  const end = source.indexOf(close, start + prefix[0].length);
  return end < 0 ? source.length : end + close.length;
}

function quotedEnd(source, start, quote) {
  let index = start + 1;
  while (index < source.length) {
    if (source[index] === "\\") index += 2;
    else if (source[index] === quote) return index + 1;
    else index += 1;
  }
  return source.length;
}

function charLiteralEnd(source, start) {
  if (source[start] !== "'") return undefined;
  const next = source[start + 1] === "\\" ? start + 3 : start + 2;
  return source[next] === "'" ? next + 1 : undefined;
}

function maskRustLexical(source, maskStrings) {
  const output = source.split("");
  let index = 0;
  while (index < source.length) {
    if (source.startsWith("//", index)) {
      const end = source.indexOf("\n", index + 2);
      const stop = end < 0 ? source.length : end;
      maskRange(output, source, index, stop);
      index = stop;
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 1;
      let end = index + 2;
      while (end < source.length && depth > 0) {
        if (source.startsWith("/*", end)) {
          depth += 1;
          end += 2;
        } else if (source.startsWith("*/", end)) {
          depth -= 1;
          end += 2;
        } else end += 1;
      }
      maskRange(output, source, index, end);
      index = end;
      continue;
    }
    const rawEnd = rawStringEnd(source, index);
    if (rawEnd !== undefined) {
      if (maskStrings) maskRange(output, source, index, rawEnd);
      index = rawEnd;
      continue;
    }
    const quoteStart = source[index] === "b" && source[index + 1] === '"' ? index + 1 : index;
    if (source[quoteStart] === '"') {
      const end = quotedEnd(source, quoteStart, '"');
      if (maskStrings) maskRange(output, source, index, end);
      index = end;
      continue;
    }
    const characterEnd = charLiteralEnd(source, index);
    if (characterEnd !== undefined) {
      if (maskStrings) maskRange(output, source, index, characterEnd);
      index = characterEnd;
      continue;
    }
    index += 1;
  }
  return output.join("");
}

export function maskRustCommentsAndStrings(source) {
  return maskRustLexical(source, true);
}

export function maskRustComments(source) {
  return maskRustLexical(source, false);
}

function matchingDelimiter(source, open, left, right) {
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === left) depth += 1;
    else if (source[index] === right && --depth === 0) return index;
  }
  return -1;
}

function nextNonSpace(source, start) {
  let index = start;
  while (index < source.length && /\s/u.test(source[index])) index += 1;
  return index;
}

function tokenAt(source, start) {
  const cursor = nextNonSpace(source, start);
  const word = source.slice(cursor).match(/^([A-Za-z_][A-Za-z0-9_]*)/u)?.[1];
  if (word) return { value: word, start: cursor, end: cursor + word.length };
  return { value: source[cursor], start: cursor, end: Math.min(source.length, cursor + 1) };
}

function macroDelimiter(source, start) {
  let cursor = start;
  const first = tokenAt(source, cursor);
  if (!/^[A-Za-z_]/u.test(first.value ?? "")) return undefined;
  cursor = first.end;
  if (first.value === "macro_rules") {
    cursor = nextNonSpace(source, cursor);
    if (source[cursor] !== "!") return undefined;
    const name = tokenAt(source, cursor + 1);
    cursor = name.end;
  } else {
    while (true) {
      cursor = nextNonSpace(source, cursor);
      if (!source.startsWith("::", cursor)) break;
      const segment = tokenAt(source, cursor + 2);
      if (!/^[A-Za-z_]/u.test(segment.value ?? "")) return undefined;
      cursor = segment.end;
    }
    cursor = nextNonSpace(source, cursor);
    if (source[cursor] !== "!") return undefined;
    cursor += 1;
  }
  cursor = nextNonSpace(source, cursor);
  return ["{", "[", "("].includes(source[cursor]) ? source[cursor] : undefined;
}

function itemKind(source, start) {
  let cursor = nextNonSpace(source, start);
  while (source.startsWith("#[", cursor)) {
    const close = matchingDelimiter(source, cursor + 1, "[", "]");
    cursor = nextNonSpace(source, close < 0 ? source.length : close + 1);
  }
  let constPrefix = false;
  while (true) {
    const token = tokenAt(source, cursor);
    if (!token.value) return { body: false, known: false };
    cursor = nextNonSpace(source, token.end);
    if (token.value === "pub" && source[cursor] === "(") {
      const close = matchingDelimiter(source, cursor, "(", ")");
      cursor = nextNonSpace(source, close < 0 ? source.length : close + 1);
      continue;
    }
    if (["pub", "async", "default", "auto", "unsafe"].includes(token.value)) continue;
    if (token.value === "const") {
      constPrefix = true;
      continue;
    }
    if (token.value === "extern") {
      const next = tokenAt(source, cursor);
      if (next.value === "crate") return { body: false, known: true };
      if (next.value === "fn" || next.value === "{") return { body: true, known: true };
      return { body: false, known: true };
    }
    if (token.value === "fn") return { body: true, known: true };
    if (["mod", "impl", "trait", "struct", "enum", "union"].includes(token.value)) return { body: true, known: true };
    if (["static", "type", "use"].includes(token.value) || constPrefix) return { body: false, known: true };
    const delimiter = macroDelimiter(source, token.start);
    if (delimiter) return { body: delimiter === "{", known: true, macroItem: true };
    return { body: false, known: false };
  }
}

function previousNonSpace(source, start) {
  let index = start - 1;
  while (index >= 0 && /\s/u.test(source[index])) index -= 1;
  return source[index];
}

function cfgTestItemEnd(source, start) {
  const kind = itemKind(source, start);
  if (!kind.known) return start;
  let parentheses = 0;
  let brackets = 0;
  let expressionBraces = 0;
  for (let index = start; index < source.length; index += 1) {
    const character = source[index];
    if (character === "!") {
      const open = nextNonSpace(source, index + 1);
      const delimiter = source[open];
      const closeToken = delimiter === "{" ? "}" : delimiter === "[" ? "]" : delimiter === "(" ? ")" : undefined;
      if (closeToken) {
        const close = matchingDelimiter(source, open, delimiter, closeToken);
        if (close < 0) return source.length;
        if (kind.macroItem && delimiter === "{") return close + 1;
        index = close;
        continue;
      }
    }
    if (character === "(") parentheses += 1;
    else if (character === ")" && parentheses > 0) parentheses -= 1;
    else if (character === "[") brackets += 1;
    else if (character === "]" && brackets > 0) brackets -= 1;
    else if (character === "{") {
      if (parentheses === 0 && brackets === 0 && expressionBraces === 0) {
        const expression = ["=", "<", "(", ",", ":", "["].includes(previousNonSpace(source, index));
        if (kind.body && !expression) {
          const close = matchingDelimiter(source, index, "{", "}");
          return close < 0 ? source.length : close + 1;
        }
      }
      expressionBraces += 1;
    } else if (character === "}" && expressionBraces > 0) expressionBraces -= 1;
    else if (character === ";" && parentheses === 0 && brackets === 0 && expressionBraces === 0) {
      return index + 1;
    }
  }
  return source.length;
}

function cfgTestRanges(maskedSource) {
  const output = maskedSource.split("");
  const ranges = [];
  const marker = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/gu;
  while (true) {
    const snapshot = output.join("");
    const match = marker.exec(snapshot);
    if (!match) return ranges;
    const stop = cfgTestItemEnd(snapshot, match.index + match[0].length);
    ranges.push([match.index, stop]);
    maskRange(output, snapshot, match.index, stop);
    marker.lastIndex = match.index;
  }
}

function applyRanges(source, ranges) {
  const output = source.split("");
  for (const [start, end] of ranges) maskRange(output, source, start, end);
  return output.join("");
}

export function maskCfgTestItems(maskedSource) {
  return applyRanges(maskedSource, cfgTestRanges(maskedSource));
}

export function productionRustSource(source) {
  const structure = maskRustCommentsAndStrings(source);
  return applyRanges(structure, cfgTestRanges(structure));
}

export function productionRustWithStrings(source) {
  const structure = maskRustCommentsAndStrings(source);
  return applyRanges(maskRustComments(source), cfgTestRanges(structure));
}
