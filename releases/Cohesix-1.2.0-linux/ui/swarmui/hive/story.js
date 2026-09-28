// Author: Lukas Bower
// Purpose: Draw evidence ownership in PixiJS using only exact verified graph bindings and source references.
// Copyright 2026 Lukas Bower

const colors = {
  queen: 0xe1be7e,
  target: 0xa2b5c9,
  worker: 0x9dcc9c,
  host: 0x7ebfa5,
  runtime: 0xc9b6d5,
  artifact: 0xd5c1a3,
  evidence: 0xc5d9b4,
};
const short = (value) => String(value || "unknown").slice(0, 16);

export function ownershipNodes(graph) {
  if (!graph?.binding || !Array.isArray(graph.records)) return [];
  const binding = graph.binding;
  const grant = graph.records.find((n) => n.kind === "grant");
  const execution = graph.records.find((n) => n.kind === "execution");
  const worker = graph.records.find((n) => n.kind === "worker");
  const nodes = [];
  if (grant)
    nodes.push({
      kind: "queen",
      label: "Queen admission",
      detail: grant.source,
      source: grant,
    });
  if (binding.target_manifest_sha256)
    nodes.push({
      kind: "target",
      label: "Target profile",
      detail: short(binding.target_manifest_sha256),
      source: {
        target_manifest_sha256: binding.target_manifest_sha256,
        proof: "Manifest binding; inspect target receipt separately",
      },
    });
  if (worker && binding.worker)
    nodes.push({
      kind: "worker",
      label: "seL4 Worker",
      detail: binding.worker.id,
      source: { ...binding.worker, receipt: worker },
    });
  if (execution)
    nodes.push({
      kind: "host",
      label: "External host",
      detail: execution.source,
      source: execution,
    });
  if (execution?.native_identity)
    nodes.push({
      kind: "runtime",
      label: "Native runtime",
      detail: short(execution.native_identity),
      source: {
        native_identity: execution.native_identity,
        record_sha256: execution.sha256,
      },
    });
  const artifacts = [
    ...new Map(
      graph.records.flatMap((n) => n.artifacts || []).map((a) => [a.sha256, a]),
    ).values(),
  ];
  if (artifacts.length)
    nodes.push({
      kind: "artifact",
      label: "CAS artifacts",
      detail: `${artifacts.length} bound objects`,
      source: artifacts,
    });
  nodes.push({
    kind: "evidence",
    label: "Causal evidence",
    detail: short(graph.graph_sha256),
    source: {
      graph_sha256: graph.graph_sha256,
      outcome: graph.outcome,
      verified_at_unix_ms: graph.verified_at_unix_ms,
    },
  });
  return nodes;
}

// This is an ownership map, not invented network topology or an execution timeline.
export class StoryCanvas {
  constructor(container, nodes, select) {
    this.container = container;
    this.nodes = nodes;
    this.select = select;
    this.app = new PIXI.Application({
      backgroundAlpha: 0,
      antialias: true,
      autoDensity: true,
      resolution: Math.min(devicePixelRatio || 1, 1.5),
    });
    this.app.ticker.stop();
    container.replaceChildren(this.app.view);
    this.app.view.setAttribute(
      "aria-label",
      "Evidence ownership map. The adjacent record buttons provide keyboard access.",
    );
    this.observer = new ResizeObserver(() => this.draw());
    this.observer.observe(container);
    this.draw();
  }
  draw() {
    const width = Math.max(1, this.container.clientWidth),
      height = Math.max(280, this.container.clientHeight);
    this.app.renderer.resize(width, height);
    for (const child of this.app.stage.removeChildren())
      child.destroy({ children: true });
    const center = { x: width / 2, y: height / 2 };
    const radius = Math.min(width * 0.34, height * 0.32);
    const ring = new PIXI.Graphics();
    ring.lineStyle(1, 0x34483a, 0.7).drawCircle(center.x, center.y, radius);
    ring
      .lineStyle(1, 0x34483a, 0.35)
      .drawCircle(center.x, center.y, radius + 21);
    this.app.stage.addChild(ring);
    this.nodes.forEach((node, index) => {
      const angle = -Math.PI / 2 + (index / this.nodes.length) * Math.PI * 2;
      const x = center.x + Math.cos(angle) * radius,
        y = center.y + Math.sin(angle) * radius;
      const group = new PIXI.Container();
      group.position.set(x, y);
      group.eventMode = "static";
      group.cursor = "pointer";
      const shape = new PIXI.Graphics(),
        color = colors[node.kind];
      shape.lineStyle(1.5, color, 0.95).beginFill(0x202b23, 1);
      if (node.kind === "queen" || node.kind === "evidence") {
        const points = Array.from({ length: 6 }, (_, n) => [
          Math.cos((n * Math.PI) / 3) * 21,
          Math.sin((n * Math.PI) / 3) * 21,
        ]).flat();
        shape.drawPolygon(points);
      } else if (["host", "target", "runtime"].includes(node.kind))
        shape.drawRoundedRect(-20, -17, 40, 34, 5);
      else if (node.kind === "artifact")
        shape.drawPolygon([0, -22, 22, 0, 0, 22, -22, 0]);
      else shape.drawCircle(0, 0, 19);
      shape.endFill();
      const dot = new PIXI.Graphics();
      dot.beginFill(color).drawCircle(0, 0, 3).endFill();
      const title = new PIXI.Text(node.label, {
        fontFamily: "Inter",
        fontSize: 12,
        fill: 0xeeeae1,
        fontWeight: "500",
      });
      title.anchor.set(0.5, 0);
      title.position.set(0, 28);
      const detail = new PIXI.Text(node.detail, {
        fontFamily: "JetBrains Mono",
        fontSize: 9,
        fill: 0xaab5a5,
      });
      detail.anchor.set(0.5, 0);
      detail.position.set(0, 46);
      group.addChild(shape, dot, title, detail);
      group.on("pointertap", () => this.select(node));
      this.app.stage.addChild(group);
    });
    const title = new PIXI.Text("ONE RUN", {
      fontFamily: "Inter",
      fontSize: 19,
      fill: 0xe1be7e,
      letterSpacing: 4,
    });
    title.anchor.set(0.5);
    title.position.set(center.x, center.y - 10);
    const subtitle = new PIXI.Text("SEPARATE OWNERS", {
      fontFamily: "JetBrains Mono",
      fontSize: 9,
      fill: 0xaab5a5,
      letterSpacing: 1,
    });
    subtitle.anchor.set(0.5);
    subtitle.position.set(center.x, center.y + 17);
    this.app.stage.addChild(title, subtitle);
    this.app.renderer.render(this.app.stage);
  }
  destroy() {
    this.observer.disconnect();
    this.app.destroy(true, { children: true });
  }
}
