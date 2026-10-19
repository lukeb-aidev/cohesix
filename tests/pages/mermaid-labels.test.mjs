/* Author: Lukas Bower
 * Purpose: Preserve Mermaid source boundaries while adapting legacy flowchart label breaks for display.
 * Copyright 2026 Lukas Bower
 */

import assert from "node:assert/strict";
import test from "node:test";
import { normalizeFlowchartLabels } from "../../assets/js/mermaid-labels.mjs";

test("normalizes quoted node and unquoted edge labels from the architecture", () => {
  const source = String.raw`flowchart TD
    ConsoleChild["Active console child\nTCP and authentication"]
    Drivers <-->|Pi GENET\n32 CPU-only pages| ConsoleChild`;
  const expected = source.replaceAll(String.raw`\n`, "<br/>");
  assert.equal(normalizeFlowchartLabels(source), expected);
  assert.ok(source.includes(String.raw`\n`), "the caller's original source remains available");
});

test("supports legacy graph headers, multiple labels and established node shapes", () => {
  const source = String.raw`graph LR; A["a\nb"] --> B(["c\nd"]) --> C{"e\nf"}
    D(("g\nh")) --> E[["i\nj"]] --> F[("k\nl")]
    G{{"m\nn"}} --> H[/"o\np"/]`;
  assert.equal(normalizeFlowchartLabels(source), source.replaceAll(String.raw`\n`, "<br/>"));
});

test("preserves comments, multiline directives and non-label tokens", () => {
  const source = String.raw`%% a comment with A["a\nb"]
%%{init: {
  "themeVariables": {"fontFamily": "a\nb"}
}}%%
flowchart TB
    click A href "https://example.com/a\nb"
    classDef sample fill:#abc
    A["real\nlabel"] %% B["comment\nlabel"]
    a\nb --> c`;
  assert.equal(normalizeFlowchartLabels(source), source.replace("real\\nlabel", "real<br/>label"));
});

test("preserves escaped backslashes and other escapes within labels", () => {
  const source = String.raw`flowchart TB
    A["a\\nb\\\nc\\\\nd\tend"] -->|C:\\new\\name| B
    C["line\nbreak"]`;
  assert.equal(normalizeFlowchartLabels(source), source.replace("line\\nbreak", "line<br/>break"));
});

test("leaves non-flowchart diagrams, ordinary text and empty input unchanged", () => {
  for (const source of [
    String.raw`sequenceDiagram
      A->>B: request\nreply`,
    String.raw`stateDiagram-v2
      A: text\ntext`,
    String.raw`not-a-diagram A["a\nb"]`,
    "",
    "%% only a comment\n",
  ]) assert.equal(normalizeFlowchartLabels(source), source);
});

test("leaves ambiguous, malformed and unrelated quoted content for Mermaid", () => {
  const source = String.raw`flowchart TD
    A["unclosed\nlabel
    B -->|"quoted\nedge"| C
    D -->|unclosed\nedge
    E["wrong\nclosing")
    style A fill:"colour\nvalue"
    arbitrary "A[\"nested\nstring\"]"`;
  assert.equal(normalizeFlowchartLabels(source), source);
});

test("keeps existing break tags and is idempotent", () => {
  const source = String.raw`flowchart TD
    A["first<br/>second\nthird"]`;
  const normalized = normalizeFlowchartLabels(source);
  assert.equal(normalized, source.replace("second\\nthird", "second<br/>third"));
  assert.equal(normalizeFlowchartLabels(normalized), normalized);
});

test("preserves LF, CRLF and CR separators exactly", () => {
  for (const newline of ["\n", "\r\n", "\r"]) {
    const source = ["flowchart TB", String.raw`A["a\nb"]`, ""].join(newline);
    assert.equal(normalizeFlowchartLabels(source), source.replace("a\\nb", "a<br/>b"));
  }
});

test("refuses non-string inputs", () => {
  assert.throws(() => normalizeFlowchartLabels(null), TypeError);
  assert.throws(() => normalizeFlowchartLabels({}), TypeError);
});
