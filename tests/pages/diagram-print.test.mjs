/* Author: Lukas Bower
 * Purpose: Verify print custody, identity and recovery of expanded documentation diagrams.
 * Copyright 2026 Lukas Bower
 */

import assert from "node:assert/strict";
import test from "node:test";
import { installDiagramPrintLifecycle } from "../../assets/js/diagram-print.mjs";

function diagram(document, open = true) {
  const figure = {};
  const dialog = { open, append(node) { node.parentElement = this; } };
  const canvas = {
    parentElement: open ? dialog : figure,
    scrollLeft: 120,
    scrollTop: 48,
    contains(node) { return node === this; },
    focus(options) { document.activeElement = this; this.focusOptions = options; },
  };
  const details = {
    before(node) {
      node.parentElement = figure;
      // Fitting a canvas back into article width can discard its expanded pan.
      node.scrollLeft = 0;
      node.scrollTop = 0;
      if (document.activeElement === node) document.activeElement = null;
    },
  };
  return { figure, dialog, canvas, details };
}

test("prints the existing canvas in its figure and restores its expanded view", () => {
  const events = new EventTarget();
  const document = { activeElement: null };
  const entry = diagram(document);
  const canvas = entry.canvas;
  document.activeElement = canvas;
  installDiagramPrintLifecycle(events, document, [entry]);

  events.dispatchEvent(new Event("beforeprint"));
  assert.equal(entry.canvas, canvas);
  assert.equal(canvas.parentElement, entry.figure);
  assert.equal(entry.dialog.open, true);
  // Some print implementations announce preparation more than once.
  events.dispatchEvent(new Event("beforeprint"));
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(canvas.parentElement, entry.dialog);
  assert.equal(canvas.scrollLeft, 120);
  assert.equal(canvas.scrollTop, 48);
  assert.equal(document.activeElement, canvas);
  assert.deepEqual(canvas.focusOptions, { preventScroll: true });
  assert.equal(entry.dialog.open, true);
});

test("closing a dialog during print keeps its canvas in the figure", () => {
  const events = new EventTarget();
  const document = { activeElement: null };
  const entry = diagram(document);
  installDiagramPrintLifecycle(events, document, [entry]);
  events.dispatchEvent(new Event("beforeprint"));
  entry.dialog.open = false;
  const expandButton = {};
  document.activeElement = expandButton;
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(entry.canvas.parentElement, entry.figure);
  assert.equal(entry.dialog.open, false);
  assert.equal(document.activeElement, expandButton);
});

test("handles multiple figures independently and ignores already inline diagrams", () => {
  const events = new EventTarget();
  const document = { activeElement: null };
  const entries = [diagram(document), diagram(document), diagram(document, false)];
  installDiagramPrintLifecycle(events, document, entries);
  events.dispatchEvent(new Event("beforeprint"));
  for (const entry of entries) assert.equal(entry.canvas.parentElement, entry.figure);
  entries[1].dialog.open = false;
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(entries[0].canvas.parentElement, entries[0].dialog);
  assert.equal(entries[1].canvas.parentElement, entries[1].figure);
  assert.equal(entries[2].canvas.parentElement, entries[2].figure);
  assert.equal(document.activeElement, null);
});

test("clears completed print state before another print or unmatched afterprint", () => {
  const events = new EventTarget();
  const document = { activeElement: null };
  const entry = diagram(document);
  installDiagramPrintLifecycle(events, document, [entry]);
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(entry.canvas.parentElement, entry.dialog);
  events.dispatchEvent(new Event("beforeprint"));
  events.dispatchEvent(new Event("afterprint"));
  entry.canvas.scrollLeft = 37;
  entry.canvas.scrollTop = 19;
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(entry.canvas.scrollLeft, 37);
  events.dispatchEvent(new Event("beforeprint"));
  events.dispatchEvent(new Event("afterprint"));
  assert.equal(entry.canvas.parentElement, entry.dialog);
  assert.equal(entry.canvas.scrollLeft, 37);
  assert.equal(entry.canvas.scrollTop, 19);
});
