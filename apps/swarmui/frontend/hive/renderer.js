import { buildHivePalette } from "./palette.js";

const createRadialTexture = (radius, innerAlpha) => {
  const canvas = document.createElement("canvas");
  const size = radius * 2;
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  const gradient = ctx.createRadialGradient(radius, radius, 0, radius, radius, radius);
  gradient.addColorStop(0, `rgba(255,255,255,${innerAlpha})`);
  gradient.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, size, size);
  return PIXI.Texture.from(canvas);
};

const ensureSprite = (map, id, factory, layer) => {
  if (map.has(id)) {
    return map.get(id);
  }
  const sprite = factory();
  map.set(id, sprite);
  layer.addChild(sprite);
  return sprite;
};

export class HiveRenderer {
  constructor(container, tokens, style, onClusterToggle, onAgentSelect) {
    this.container = container;
    this.tokens = tokens;
    this.style = style;
    this.palette = buildHivePalette(tokens);
    this.onClusterToggle = onClusterToggle;
    this.onAgentSelect = onAgentSelect;
    this.view = { zoom: 1, panX: 0, panY: 0 };
    this.agentSprites = new Map();
    this.glowSprites = new Map();
    this.agentLabels = new Map();
    this.clusterNodes = new Map();
    this.pollenPool = [];
    this.pulsePool = [];
    this.flowPool = [];
    this.selectedAgent = null;
    this.app = new PIXI.Application({
      backgroundAlpha: 0,
      antialias: true,
      autoDensity: true,
      resolution: window.devicePixelRatio || 1,
    });
    this.app.ticker.stop();
    container.innerHTML = "";
    container.appendChild(this.app.view);
    this.root = new PIXI.Container();
    this.flowLayer = new PIXI.Container();
    this.clusterLayer = new PIXI.Container();
    this.heatLayer = new PIXI.Container();
    this.agentLayer = new PIXI.Container();
    this.labelLayer = new PIXI.Container();
    this.pollenLayer = new PIXI.Container();
    this.pulseLayer = new PIXI.Container();
    this.root.addChild(this.flowLayer);
    this.root.addChild(this.heatLayer);
    this.root.addChild(this.clusterLayer);
    this.root.addChild(this.pollenLayer);
    this.root.addChild(this.agentLayer);
    this.root.addChild(this.labelLayer);
    this.root.addChild(this.pulseLayer);
    this.app.stage.addChild(this.root);
    this.agentTexture = this.buildCircleTexture(this.style.agentRadius);
    this.pollenTexture = this.buildCircleTexture(this.style.pollenRadius);
    this.glowTexture = createRadialTexture(this.style.glowRadius, 0.8);
    this.pulseTexture = createRadialTexture(this.style.pulseRadius, 0.9);
    this.flowTexture = createRadialTexture(
      this.style.flowBlobRadius,
      this.style.flowBlobInnerAlpha,
    );
    this.needsResize = true;
    this.attachInteraction();
    this.attachResizeObserver();
    this.resizeIfNeeded();
  }

  buildCircleTexture(radius) {
    const g = new PIXI.Graphics();
    g.beginFill(0xffffff);
    g.drawCircle(radius, radius, radius);
    g.endFill();
    return this.app.renderer.generateTexture(g);
  }

  attachInteraction() {
    let dragging = false;
    let last = { x: 0, y: 0 };
    const view = this.app.view;
    view.addEventListener("wheel", (event) => {
      event.preventDefault();
      const delta = event.deltaY < 0 ? 0.1 : -0.1;
      const next = Math.min(
        this.style.zoomMax,
        Math.max(this.style.zoomMin, this.view.zoom + delta),
      );
      this.view.zoom = next;
    }, { passive: false });
    view.addEventListener("pointerdown", (event) => {
      dragging = true;
      last = { x: event.clientX, y: event.clientY };
    });
    view.addEventListener("pointerup", () => {
      dragging = false;
    });
    view.addEventListener("pointerleave", () => {
      dragging = false;
    });
    view.addEventListener("pointermove", (event) => {
      if (!dragging) {
        return;
      }
      const dx = event.clientX - last.x;
      const dy = event.clientY - last.y;
      last = { x: event.clientX, y: event.clientY };
      this.view.panX += dx;
      this.view.panY += dy;
    });
  }

  attachResizeObserver() {
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", () => {
        this.needsResize = true;
      });
      return;
    }
    this.resizeObserver = new ResizeObserver(() => {
      this.needsResize = true;
    });
    this.resizeObserver.observe(this.container);
  }

  resizeIfNeeded() {
    if (!this.needsResize) {
      return;
    }
    const rect = this.container.getBoundingClientRect();
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    if (this.width === width && this.height === height) {
      this.needsResize = false;
      return;
    }
    this.width = width;
    this.height = height;
    this.app.renderer.resize(this.width, this.height);
    this.needsResize = false;
  }

  resetView() {
    this.view.zoom = 1;
    this.view.panX = 0;
    this.view.panY = 0;
  }

  render(world, lodMode) {
    this.resizeIfNeeded();
    this.root.position.set(this.width / 2 + this.view.panX, this.height / 2 + this.view.panY);
    this.root.scale.set(this.view.zoom);
    world.setBounds(this.width, this.height);
    this.drawFlows(world, lodMode);
    this.drawClusters(world, lodMode);
    this.drawAgents(world, lodMode);
    this.drawPollen(world, lodMode);
    this.drawPulses(world);
  }

  draw() {
    this.app.renderer.render(this.app.stage);
  }

  drawFlows(world, lodMode) {
    let used = 0;
    const spacingScale = lodMode === "degraded" ? 1.6 : lodMode === "cluster" ? 1.2 : 1;
    const spacing = this.style.flowBlobSpacing * spacingScale;
    for (const flow of world.flows.values()) {
      if (lodMode === "detail" && flow.intensity < this.style.flowDetailThreshold) {
        continue;
      }
      const source = world.clusters.get(flow.source);
      const target = world.clusters.get(flow.target) || world.clusters.get("queen");
      if (!source || !target) {
        continue;
      }
      const dx = target.center.x - source.center.x;
      const dy = target.center.y - source.center.y;
      const distance = Math.max(1, Math.hypot(dx, dy));
      const count = Math.max(
        1,
        Math.min(this.style.flowBlobLimit, Math.floor(distance / spacing)),
      );
      const strength = clamp(flow.intensity / this.style.flowIntensityMax, 0, 1);
      const alpha = this.style.flowBlobAlphaMin + strength * this.style.flowBlobAlphaRange;
      const scale = this.style.flowBlobScaleMin + strength * this.style.flowBlobScaleRange;
      for (let idx = 0; idx < count; idx += 1) {
        const t = count === 1 ? 0.5 : idx / (count - 1);
        const sprite = this.flowPool[used] || new PIXI.Sprite(this.flowTexture);
        if (!this.flowPool[used]) {
          sprite.anchor.set(0.5);
          sprite.blendMode = PIXI.BLEND_MODES.ADD;
          this.flowPool[used] = sprite;
          this.flowLayer.addChild(sprite);
        }
        sprite.visible = true;
        sprite.tint = this.palette.flow;
        sprite.alpha = alpha;
        sprite.scale.set(scale);
        sprite.position.set(source.center.x + dx * t, source.center.y + dy * t);
        used += 1;
      }
    }
    for (let idx = used; idx < this.flowPool.length; idx += 1) {
      this.flowPool[idx].visible = false;
    }
  }

  drawClusters(world, lodMode) {
    const existing = new Set();
    for (const cluster of world.clusters.values()) {
      existing.add(cluster.id);
      let node = this.clusterNodes.get(cluster.id);
      if (!node) {
        const hull = new PIXI.Graphics();
        hull.eventMode = "static";
        hull.cursor = "pointer";
        hull.on("pointertap", () => {
          if (this.onClusterToggle) {
            this.onClusterToggle(cluster.id);
          }
        });
        const label = new PIXI.Text(cluster.id, {
          fontFamily: this.style.labelFont,
          fontSize: this.style.clusterLabelSize,
          fill: this.palette.label,
        });
        label.anchor.set(0.5, 0.5);
        const container = new PIXI.Container();
        container.addChild(hull);
        container.addChild(label);
        this.clusterLayer.addChild(container);
        node = { container, hull, label };
        this.clusterNodes.set(cluster.id, node);
      }
      node.hull.clear();
      const alpha = cluster.collapsed
        ? this.style.clusterAlphaCollapsed
        : this.style.clusterAlpha;
      node.hull.lineStyle(1, this.palette.cluster, alpha);
      node.hull.drawCircle(cluster.center.x, cluster.center.y, cluster.radius);
      node.label.text = cluster.id;
      node.label.position.set(
        cluster.center.x,
        cluster.center.y - cluster.radius - this.style.clusterLabelOffset,
      );
    }
    for (const [id, node] of this.clusterNodes.entries()) {
      if (!existing.has(id)) {
        this.clusterLayer.removeChild(node.container);
        node.container.destroy({ children: true });
        this.clusterNodes.delete(id);
      }
    }
  }

  roleTint(role) {
    switch (String(role || "").toLowerCase()) {
      case "queen":
        return this.palette.queen;
      case "worker-gpu":
        return this.palette.workerGpu;
      case "worker-lora":
        return this.palette.workerLora;
      case "worker-bus":
        return this.palette.workerBus;
      case "worker-heartbeat":
      case "worker-heart":
        return this.palette.workerHeart;
      default:
        return this.palette.worker;
    }
  }

  setSelectedAgent(agentId) {
    this.selectedAgent = agentId;
  }

  drawAgents(world, lodMode) {
    const showAgents = lodMode === "detail" || lodMode === "balanced";
    const existing = new Set();
    for (const [id, agent] of world.agents.entries()) {
      existing.add(id);
      const cluster = world.clusters.get(agent.cluster);
      const collapsed = cluster && cluster.collapsed && agent.role !== "queen";
      if ((!showAgents && agent.role !== "queen") || collapsed) {
        const sprite = this.agentSprites.get(id);
        const glow = this.glowSprites.get(id);
        const label = this.agentLabels.get(id);
        if (sprite) sprite.visible = false;
        if (glow) glow.visible = false;
        if (label) label.visible = false;
        continue;
      }
      const position = world.positionForAgent(agent);
      const sprite = ensureSprite(
        this.agentSprites,
        id,
        () => {
          const s = new PIXI.Sprite(this.agentTexture);
          s.anchor.set(0.5);
          s.eventMode = "static";
          s.cursor = "pointer";
          s.on("pointertap", () => {
            if (this.onAgentSelect) {
              this.onAgentSelect(id);
            }
          });
          return s;
        },
        this.agentLayer,
      );
      const glow = ensureSprite(
        this.glowSprites,
        id,
        () => {
          const s = new PIXI.Sprite(this.glowTexture);
          s.anchor.set(0.5);
          s.blendMode = PIXI.BLEND_MODES.ADD;
          return s;
        },
        this.heatLayer,
      );
      sprite.visible = true;
      glow.visible = true;
      sprite.position.set(position.x, position.y);
      const roleTint = this.roleTint(agent.role);
      sprite.eventMode = agent.role === "queen" ? "none" : "static";
      sprite.cursor = agent.role === "queen" ? "default" : "pointer";
      sprite.tint = agent.error > this.style.errorTintThreshold
        ? this.palette.error
        : roleTint;
      const scale = this.style.agentScaleBase + agent.heat * this.style.agentScaleHeat;
      const selectedScale = id === this.selectedAgent ? 1.1 : 1.0;
      sprite.scale.set(scale * selectedScale);
      glow.position.set(position.x, position.y);
      glow.tint = agent.error > this.style.errorGlowThreshold
        ? this.palette.error
        : roleTint;
      glow.alpha = clamp(
        agent.heat + agent.error * this.style.glowErrorBoost,
        this.style.glowAlphaMin,
        this.style.glowAlphaMax,
      );
      glow.scale.set(this.style.glowScaleBase + agent.heat * this.style.glowScaleHeat);
      if (agent.role !== "queen") {
        const label = ensureSprite(
          this.agentLabels,
          id,
          () => {
            const text = new PIXI.Text("", {
              fontFamily: this.style.labelFont,
              fontSize: this.style.agentLabelSize,
              fill: this.palette.label,
            });
            text.anchor.set(0.5);
            return text;
          },
          this.labelLayer,
        );
        label.visible = true;
        label.text = agent.labelIndex ? String(agent.labelIndex) : "";
        label.position.set(
          position.x + this.style.agentLabelOffset,
          position.y - this.style.agentLabelOffset,
        );
      } else {
        const label = this.agentLabels.get(id);
        if (label) {
          label.visible = false;
        }
      }
    }
    for (const [id, sprite] of this.agentSprites.entries()) {
      if (!existing.has(id)) {
        this.agentLayer.removeChild(sprite);
        sprite.destroy();
        this.agentSprites.delete(id);
      }
    }
    for (const [id, glow] of this.glowSprites.entries()) {
      if (!existing.has(id)) {
        this.heatLayer.removeChild(glow);
        glow.destroy();
        this.glowSprites.delete(id);
      }
    }
    for (const [id, label] of this.agentLabels.entries()) {
      if (!existing.has(id)) {
        this.labelLayer.removeChild(label);
        label.destroy();
        this.agentLabels.delete(id);
      }
    }
  }

  drawPollen(world, lodMode) {
    const show = lodMode === "detail";
    const pollen = world.pollen;
    const limit = Math.min(pollen.length, world.maxPollen);
    for (let idx = 0; idx < limit; idx += 1) {
      const particle = pollen[idx];
      const sprite = this.pollenPool[idx] || new PIXI.Sprite(this.pollenTexture);
      if (!this.pollenPool[idx]) {
        sprite.anchor.set(0.5);
        this.pollenPool[idx] = sprite;
        this.pollenLayer.addChild(sprite);
      }
      sprite.visible = show;
      sprite.position.set(particle.x, particle.y);
      sprite.alpha = clamp(1 - particle.age / particle.life, 0, 1);
      sprite.tint = this.palette.pollen;
    }
    for (let idx = limit; idx < this.pollenPool.length; idx += 1) {
      this.pollenPool[idx].visible = false;
    }
  }

  drawPulses(world) {
    const pulses = world.pulses;
    const limit = Math.min(pulses.length, world.maxPulses);
    for (let idx = 0; idx < limit; idx += 1) {
      const pulse = pulses[idx];
      const sprite = this.pulsePool[idx] || new PIXI.Sprite(this.pulseTexture);
      if (!this.pulsePool[idx]) {
        sprite.anchor.set(0.5);
        sprite.blendMode = PIXI.BLEND_MODES.ADD;
        this.pulsePool[idx] = sprite;
        this.pulseLayer.addChild(sprite);
      }
      sprite.visible = true;
      const scale = this.style.pulseScaleBase
        + (pulse.age / pulse.life) * this.style.pulseScaleRange;
      sprite.position.set(pulse.x, pulse.y);
      sprite.scale.set(scale);
      sprite.alpha = clamp(1 - pulse.age / pulse.life, 0, 1);
      sprite.tint = this.palette.error;
    }
    for (let idx = limit; idx < this.pulsePool.length; idx += 1) {
      this.pulsePool[idx].visible = false;
    }
  }

  destroy() {
    if (this.resizeObserver) {
      this.resizeObserver.disconnect();
    }
    this.app.destroy(true, { children: true });
  }

  getAgentScreenPositions() {
    const rect = this.app.view.getBoundingClientRect();
    const positions = [];
    for (const [id, sprite] of this.agentSprites.entries()) {
      if (!sprite.visible) {
        continue;
      }
      const pos = sprite.getGlobalPosition();
      positions.push({
        id,
        x: rect.left + pos.x,
        y: rect.top + pos.y,
      });
    }
    return positions;
  }

  getAgentStates() {
    const states = [];
    for (const [id, sprite] of this.agentSprites.entries()) {
      states.push({
        id,
        visible: sprite.visible,
        tint: sprite.tint,
      });
    }
    return states;
  }

  getAgentLabels() {
    const labels = [];
    for (const [id, label] of this.agentLabels.entries()) {
      labels.push({
        id,
        visible: label.visible,
        text: label.text,
      });
    }
    return labels;
  }
}

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
