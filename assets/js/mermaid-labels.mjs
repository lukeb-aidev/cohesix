/* Author: Lukas Bower
 * Purpose: Adapt legacy flowchart label breaks for display without changing canonical Mermaid source.
 * Copyright 2026 Lukas Bower
 */

const NODE_SHAPES = new Map([
  ["[", "]"], ["(", ")"], ["{", "}"], [">", "]"],
  ["[[", "]]"], ["((", "))"], ["(((", ")))"], ["{{", "}}"],
  ["([", "])"], ["[(", ")]"], ["[/", "/]"], ["[\\", "\\]"],
]);
const EDGE_END = /(?:<-->|<==>|<-\.->|-->|==>|-\.->|---|===|--[ox]|[ox]--[ox])\s*$/;

function labelBreaks(label) {
  // A doubled backslash may represent a path or an intentionally literal escape.
  // Leave the whole run unchanged rather than interpreting a second grammar.
  return label.replace(/\\+n/g, (escape) => escape.length === 2 ? "<br/>" : escape);
}

function quotedEnd(line, start) {
  for (let index = start + 1; index < line.length; index += 1) {
    if (line[index] === "\\") index += 1;
    else if (line[index] === '"') return index;
  }
  return -1;
}

function nodeClosing(line, quote) {
  const prefix = line.slice(0, quote);
  const match = /(?:^|[^A-Za-z0-9_-])([A-Za-z_][A-Za-z0-9_-]*)\s*([\[({>/\\]+)\s*$/.exec(prefix);
  return match ? NODE_SHAPES.get(match[2]) : undefined;
}

function normalizeLine(line) {
  let result = "";
  let cursor = 0;
  for (let index = 0; index < line.length; index += 1) {
    if (line.startsWith("%%", index)) break;
    if (line[index] === '"') {
      const end = quotedEnd(line, index);
      if (end < 0) break;
      const closing = nodeClosing(line, index);
      if (closing && line.slice(end + 1).trimStart().startsWith(closing)) {
        result += line.slice(cursor, index + 1) + labelBreaks(line.slice(index + 1, end));
        cursor = end;
      }
      index = end;
    } else if (line[index] === "|" && EDGE_END.test(line.slice(0, index))) {
      let end = index + 1;
      for (; end < line.length; end += 1) {
        if (line[end] === "\\") end += 1;
        else if (line[end] === "|") break;
      }
      if (end >= line.length) break;
      const label = line.slice(index + 1, end);
      if (!label.trimStart().startsWith('"')) {
        result += line.slice(cursor, index + 1) + labelBreaks(label);
        cursor = end;
      }
      index = end;
    }
  }
  return result + line.slice(cursor);
}

/**
 * Normalize display-only breaks in a conservative subset of flowchart labels.
 *
 * Canonical source and its fallback must retain the caller's original string.
 * Unsupported syntax passes through unchanged for Mermaid to parse or refuse.
 */
export function normalizeFlowchartLabels(source) {
  if (typeof source !== "string") throw new TypeError("Mermaid source must be a string");
  const parts = source.split(/(\r\n|\r|\n)/);
  let flowchart = false;
  let foundHeader = false;
  let directive = false;
  for (let index = 0; index < parts.length; index += 2) {
    const line = parts[index];
    const trimmed = line.trimStart();
    if (directive) {
      if (trimmed.includes("}%%")) directive = false;
      continue;
    }
    if (trimmed.startsWith("%%{")) {
      directive = !trimmed.includes("}%%");
      continue;
    }
    if (!trimmed || trimmed.startsWith("%%")) continue;
    if (!foundHeader) {
      foundHeader = true;
      flowchart = /^(?:flowchart|graph)(?:[ \t]+(?:TB|TD|BT|RL|LR))?(?=[ \t;]|$)/.test(trimmed);
      if (!flowchart) return source;
    }
    parts[index] = normalizeLine(line);
  }
  return flowchart ? parts.join("") : source;
}
