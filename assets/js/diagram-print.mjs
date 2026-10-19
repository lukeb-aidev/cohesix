/* Author: Lukas Bower
 * Purpose: Preserve custody and the expanded view of each Mermaid canvas across browser printing.
 * Copyright 2026 Lukas Bower
 */

/** Keep the sole canvas in its figure for print, without closing its dialog. */
export function installDiagramPrintLifecycle(eventTarget, document, entries) {
  const expandedViews = new Map();

  eventTarget.addEventListener("beforeprint", () => {
    for (const entry of entries) {
      if (!entry.dialog.open || entry.canvas.parentElement !== entry.dialog) continue;
      expandedViews.set(entry, {
        left: entry.canvas.scrollLeft,
        top: entry.canvas.scrollTop,
        focus: entry.canvas.contains(document.activeElement) ? document.activeElement : null,
      });
      entry.details.before(entry.canvas);
    }
  });

  eventTarget.addEventListener("afterprint", () => {
    for (const [entry, view] of expandedViews) {
      // Closing during print returns ownership to the figure permanently.
      if (!entry.dialog.open || entry.canvas.parentElement !== entry.figure) continue;
      entry.dialog.append(entry.canvas);
      entry.canvas.scrollLeft = view.left;
      entry.canvas.scrollTop = view.top;
      view.focus?.focus({ preventScroll: true });
    }
    expandedViews.clear();
  });
}
