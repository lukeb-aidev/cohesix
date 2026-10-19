/* Author: Lukas Bower
 * Purpose: Enhance generated documentation navigation, tables and Mermaid with readable failure fallbacks.
 * Copyright 2026 Lukas Bower
 */
"use strict";

import { normalizeFlowchartLabels } from "./mermaid-labels.mjs";
import { installDiagramPrintLifecycle } from "./diagram-print.mjs";

(() => {
  const article = document.querySelector("#document-content");
  if (!article) return;

  const disclosure = document.querySelector(".nav-disclosure");
  const narrow = window.matchMedia("(max-width: 960px)");
  if (disclosure) {
    const resizeNavigation = () => { disclosure.open = !narrow.matches; };
    resizeNavigation();
    narrow.addEventListener("change", resizeNavigation);
  }

  for (const table of article.querySelectorAll("table")) {
    const region = document.createElement("div");
    region.className = "table-scroll";
    region.tabIndex = 0;
    region.setAttribute("role", "region");
    region.setAttribute("aria-label", "Table; scroll horizontally if needed");
    table.before(region);
    region.append(table);
  }

  const outline = document.querySelector("#page-outline");
  const headings = article.querySelectorAll("h2[id]");
  if (outline && headings.length > 1) {
    const label = document.createElement("p");
    label.className = "nav-label";
    label.textContent = "On this page";
    const list = document.createElement("ul");
    for (const heading of headings) {
      const item = document.createElement("li");
      const link = document.createElement("a");
      link.href = `#${encodeURIComponent(heading.id)}`;
      link.textContent = heading.textContent;
      item.append(link);
      list.append(item);
    }
    outline.append(label, list);
    outline.parentElement.hidden = false;
  }

  const diagrams = Array.from(article.querySelectorAll("pre > code.language-mermaid"));
  if (!diagrams.length) return;

  // Source remains in the document throughout loading, parsing and rendering.
  // A failure in one fence must not prevent another diagram from rendering.
  const entries = diagrams.map((code, index) => {
    const source = code.textContent;
    const figure = document.createElement("figure");
    figure.className = "diagram";
    const caption = document.createElement("figcaption");
    caption.className = "diagram-toolbar";
    caption.textContent = "Diagram";
    const expand = document.createElement("button");
    expand.type = "button";
    expand.textContent = "Expand diagram";
    expand.hidden = true;
    caption.append(expand);
    const canvas = document.createElement("div");
    canvas.className = "diagram-canvas";
    canvas.setAttribute("role", "region");
    canvas.setAttribute("aria-label", "Mermaid diagram; scroll horizontally if needed");
    canvas.tabIndex = 0;
    canvas.hidden = true;
    const details = document.createElement("details");
    details.className = "diagram-source";
    details.open = true;
    const summary = document.createElement("summary");
    summary.textContent = "Diagram source";
    const pre = code.parentElement;
    pre.before(figure);
    details.append(summary, pre);
    figure.append(caption, canvas, details);
    const dialog = document.createElement("dialog");
    dialog.className = "diagram-dialog";
    dialog.setAttribute("aria-label", "Expanded Mermaid diagram");
    const toolbar = document.createElement("div");
    toolbar.className = "diagram-toolbar";
    const title = document.createElement("span");
    title.textContent = "Diagram · scroll to explore";
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "Close diagram";
    close.addEventListener("click", () => dialog.close());
    toolbar.append(title, close);
    dialog.append(toolbar);
    document.body.append(dialog);
    expand.addEventListener("click", () => {
      dialog.append(canvas);
      dialog.showModal();
      close.focus();
    });
    dialog.addEventListener("close", () => {
      details.before(canvas);
      expand.focus();
    });
    return { source, figure, canvas, details, dialog, expand, id: `cohesix-diagram-${index}` };
  });
  installDiagramPrintLifecycle(window, document, entries);

  const showError = (entry, message) => {
    const status = document.createElement("p");
    status.className = "diagram-error";
    status.setAttribute("role", "status");
    status.textContent = message;
    entry.figure.prepend(status);
    entry.details.open = true;
    entry.figure.dataset.diagramState = "error";
  };

  const loadMermaid = () => new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js";
    script.integrity = "sha384-o+g/BxPwhi0C3RK7oQBxQuNimeafQ3GE/ST4iT2BxVI4Wzt60SH4pq9iXVYujjaS";
    script.crossOrigin = "anonymous";
    script.async = true;
    const timer = window.setTimeout(() => {
      script.remove();
      reject(new Error("Diagram renderer timed out"));
    }, 15000);
    script.onload = () => {
      window.clearTimeout(timer);
      if (window.mermaid) resolve(window.mermaid);
      else reject(new Error("Diagram renderer unavailable"));
    };
    script.onerror = () => {
      window.clearTimeout(timer);
      reject(new Error("Diagram renderer could not load"));
    };
    document.head.append(script);
  });

  const renderDiagrams = async () => {
    let mermaid;
    try {
      mermaid = await loadMermaid();
      // Font loading precedes SVG measurement; fallback fonts still work offline.
      await document.fonts.ready;
      const style = getComputedStyle(document.documentElement);
      const token = (name) => style.getPropertyValue(name).trim();
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        htmlLabels: false,
        suppressErrorRendering: true,
        secure: ["secure", "securityLevel", "startOnLoad", "maxTextSize", "maxEdges", "suppressErrorRendering", "theme", "themeVariables", "fontFamily", "htmlLabels"],
        theme: "base",
        fontFamily: 'Inter, "Helvetica Neue", sans-serif',
        themeVariables: {
          darkMode: true,
          background: token("--palette-void"),
          primaryColor: token("--palette-surface-2"),
          primaryTextColor: token("--palette-ink"),
          primaryBorderColor: token("--palette-ember"),
          secondaryColor: token("--palette-void-mid"),
          secondaryTextColor: token("--palette-ink"),
          secondaryBorderColor: token("--palette-cyan"),
          tertiaryColor: token("--palette-surface"),
          tertiaryTextColor: token("--palette-ink"),
          tertiaryBorderColor: token("--palette-border"),
          lineColor: token("--palette-ink-soft"),
          textColor: token("--palette-ink"),
          mainBkg: token("--palette-surface-2"),
          nodeBorder: token("--palette-ember"),
          clusterBkg: token("--palette-surface"),
          clusterBorder: token("--palette-border"),
          edgeLabelBackground: token("--palette-void"),
          actorBkg: token("--palette-surface-2"),
          actorBorder: token("--palette-ember"),
          actorTextColor: token("--palette-ink"),
          signalColor: token("--palette-ink"),
          signalTextColor: token("--palette-ink"),
          labelBoxBkgColor: token("--palette-surface"),
          labelBoxBorderColor: token("--palette-border"),
          labelTextColor: token("--palette-ink"),
          loopTextColor: token("--palette-ink"),
          noteBkgColor: token("--palette-void-mid"),
          noteTextColor: token("--palette-ink"),
          noteBorderColor: token("--palette-cyan")
        },
        flowchart: { htmlLabels: false, useMaxWidth: false },
        sequence: { useMaxWidth: false },
        state: { useMaxWidth: false }
      });
    } catch {
      for (const entry of entries) {
        showError(entry, "The diagram renderer could not load. The original source is available below.");
      }
      return;
    }

    for (const entry of entries) {
      try {
        const result = await mermaid.render(entry.id, normalizeFlowchartLabels(entry.source));
        entry.canvas.innerHTML = result.svg;
        entry.canvas.hidden = false;
        entry.details.open = false;
        entry.expand.hidden = false;
        entry.figure.dataset.diagramState = "rendered";
      } catch {
        showError(entry, "This diagram could not render. The original source is available below.");
      }
    }
  };

  renderDiagrams();
})();
