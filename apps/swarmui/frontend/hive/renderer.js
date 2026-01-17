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
  constructor(container, tokens, style, onClusterToggle) {
    this.container = container;
    this.tokens = tokens;
    this.style = style;
    this.palette = buildHivePalette(tokens);
    this.onClusterToggle = onClusterToggle;
    this.view = { zoom: 1, panX: 0, panY: 0 };
    this.agentSprites = new Map();
    this.glowSprites = new Map();
    this.clusterNodes = new Map();
    this.pollenPool = [];
    this.pulsePool = [];
    this.flowPool = [];
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
    this.pollenLayer = new PIXI.Container();
    this.pulseLayer = new PIXI.Container();
    this.root.addChild(this.flowLayer);
    this.root.addChild(this.heatLayer);
    this.root.addChild(this.clusterLayer);
    this.root.addChild(this.pollenLayer);
    this.root.addChild(this.agentLayer);
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
    this.attachInteraction();
    this.resize();
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

  resize() {
    const rect = this.container.getBoundingClientRect();
    this.width = rect.width;
    this.height = rect.height;
    this.app.renderer.resize(this.width, this.height);
  }

  resetView() {
    this.view.zoom = 1;
    this.view.panX = 0;
    this.view.panY = 0;
  }

  render(world, lodMode) {
    this.resize();
    this.root.position.set(this.width / 2 + this.view.panX, this.height / 2 + this.view.panY);
    this.root.scale.set(this.view.zoom);
    world.setBounds(this.width, this.height);
    this.drawFlows(world, lodMode);
    this.drawClusters(world, lodMode);
    this.drawAgents(world, lodMode);
    this.drawPollen(world, lodMode);
    this.drawPulses(world);
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

  drawAgents(world, lodMode) {
    const showAgents = lodMode === "detail" || lodMode === "balanced";
    for (const [id, agent] of world.agents.entries()) {
      const cluster = world.clusters.get(agent.cluster);
      const collapsed = cluster && cluster.collapsed && agent.role !== "queen";
      if ((!showAgents && agent.role !== "queen") || collapsed) {
        const sprite = this.agentSprites.get(id);
        const glow = this.glowSprites.get(id);
        if (sprite) sprite.visible = false;
        if (glow) glow.visible = false;
        continue;
      }
      const position = world.positionForAgent(agent);
      const sprite = ensureSprite(
        this.agentSprites,
        id,
        () => {
          const s = new PIXI.Sprite(this.agentTexture);
          s.anchor.set(0.5);
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
      sprite.tint = agent.error > this.style.errorTintThreshold
        ? this.palette.error
        : this.palette.agent;
      const scale = this.style.agentScaleBase + agent.heat * this.style.agentScaleHeat;
      sprite.scale.set(scale);
      glow.position.set(position.x, position.y);
      glow.tint = agent.error > this.style.errorGlowThreshold
        ? this.palette.error
        : this.palette.heat;
      glow.alpha = clamp(
        agent.heat + agent.error * this.style.glowErrorBoost,
        this.style.glowAlphaMin,
        this.style.glowAlphaMax,
      );
      glow.scale.set(this.style.glowScaleBase + agent.heat * this.style.glowScaleHeat);
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
    this.app.destroy(true, { children: true });
  }
}

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
