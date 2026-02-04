const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

const hashString = (value) => {
  let hash = FNV_OFFSET;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, FNV_PRIME);
  }
  return hash >>> 0;
};

const seededUnit = (hash, shift) => ((hash >>> shift) & 0xff) / 255;

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

const clusterKeyFromNamespace = (namespace) => {
  const parts = namespace.split("/").filter(Boolean);
  if (parts.length === 0) {
    return "root";
  }
  if (parts[0] === "shard" && parts.length > 2) {
    return `shard/${parts[1]}/${parts[2]}`;
  }
  return parts[0];
};

const parseWorkerIndex = (id) => {
  const dash = id.lastIndexOf("-");
  if (dash === -1 || dash === id.length - 1) {
    return null;
  }
  const suffix = id.slice(dash + 1);
  if (!/^\d+$/.test(suffix)) {
    return null;
  }
  const value = Number.parseInt(suffix, 10);
  return Number.isFinite(value) ? value : null;
};

export class HiveWorld {
  constructor(style) {
    this.style = style;
    this.time = 0;
    this.bounds = { width: 900, height: 600 };
    this.agents = new Map();
    this.labelDirty = false;
    this.pollen = [];
    this.pulses = [];
    this.flows = new Map();
    this.clusters = new Map();
    this.maxPollen = style.maxPollen;
    this.maxPulses = style.maxPulses;
  }

  setBounds(width, height) {
    this.bounds.width = width;
    this.bounds.height = height;
  }

  ensureAgent(id, namespace, role = "worker") {
    const existing = this.agents.get(id);
    if (existing) {
      if (namespace) {
        existing.namespace = namespace;
        existing.cluster = clusterKeyFromNamespace(namespace);
      }
      if (role) {
        existing.role = role;
      }
      return existing;
    }
    const seed = hashString(id);
    const anchor = {
      x: seededUnit(seed, 0),
      y: seededUnit(seed, 8),
    };
    const agent = {
      id,
      role,
      namespace,
      cluster: clusterKeyFromNamespace(namespace || "/"),
      seed,
      anchor,
      heat: 0,
      error: 0,
      labelIndex: null,
    };
    this.agents.set(id, agent);
    this.registerCluster(agent.cluster, id);
    this.labelDirty = true;
    return agent;
  }

  registerCluster(clusterId, agentId) {
    if (!this.clusters.has(clusterId)) {
      this.clusters.set(clusterId, {
        id: clusterId,
        collapsed: false,
        members: new Set(),
        center: { x: 0, y: 0 },
        radius: 0,
      });
    }
    const cluster = this.clusters.get(clusterId);
    cluster.members.add(agentId);
  }

  toggleCluster(clusterId) {
    const cluster = this.clusters.get(clusterId);
    if (!cluster) {
      return;
    }
    cluster.collapsed = !cluster.collapsed;
  }

  emitTelemetry(agent, intensity, allowParticles) {
    agent.heat = clamp(agent.heat + intensity, 0, 1);
    const target = this.agents.get("queen");
    const sourcePos = this.positionForAgent(agent);
    const targetPos = target ? this.positionForAgent(target) : { x: 0, y: 0 };
    this.bumpFlow(agent.cluster, "queen", intensity);
    if (allowParticles && this.pollen.length < this.maxPollen) {
      this.spawnPollen(sourcePos, targetPos, intensity);
    }
  }

  emitError(agent) {
    agent.error = 1;
    const pos = this.positionForAgent(agent);
    if (this.pulses.length >= this.maxPulses) {
      this.pulses.shift();
    }
    this.pulses.push({
      x: pos.x,
      y: pos.y,
      age: 0,
      life: this.style.pulseLife,
    });
    this.bumpFlow(agent.cluster, "queen", this.style.flowIntensityError);
  }

  spawnPollen(source, target, intensity) {
    const dx = target.x - source.x;
    const dy = target.y - source.y;
    const length = Math.max(1, Math.hypot(dx, dy));
    const speed = this.style.pollenSpeedBase + intensity * this.style.pollenSpeedScale;
    this.pollen.push({
      x: source.x,
      y: source.y,
      vx: (dx / length) * speed,
      vy: (dy / length) * speed,
      age: 0,
      life: this.style.pollenLife,
    });
  }

  bumpFlow(sourceCluster, targetCluster, intensity) {
    const key = `${sourceCluster}->${targetCluster}`;
    const existing = this.flows.get(key) || {
      source: sourceCluster,
      target: targetCluster,
      intensity: 0,
    };
    existing.intensity = clamp(
      existing.intensity + intensity,
      0,
      this.style.flowIntensityMax,
    );
    this.flows.set(key, existing);
  }

  positionForAgent(agent) {
    const { width, height } = this.bounds;
    const anchorX = (agent.anchor.x - 0.5) * width * this.style.positionScale;
    const anchorY = (agent.anchor.y - 0.5) * height * this.style.positionScale;
    const wobble = this.style.driftAmplitude;
    const phase = (agent.seed % 360) * (Math.PI / 180);
    const wobbleX = Math.sin(this.time * this.style.driftRateX + phase) * wobble;
    const wobbleY = Math.cos(this.time * this.style.driftRateY + phase) * wobble;
    return {
      x: anchorX + wobbleX,
      y: anchorY + wobbleY,
    };
  }

  update(dt) {
    this.time += dt;
    for (const agent of this.agents.values()) {
      agent.heat = clamp(agent.heat - dt * this.style.heatDecay, 0, 1);
      agent.error = clamp(agent.error - dt * this.style.errorDecay, 0, 1);
    }
    for (const [key, flow] of this.flows.entries()) {
      flow.intensity = clamp(
        flow.intensity - dt * this.style.flowDecay,
        0,
        this.style.flowIntensityMax,
      );
      if (flow.intensity <= this.style.flowInactiveThreshold) {
        this.flows.delete(key);
      }
    }
    this.pollen = this.pollen.filter((particle) => {
      particle.age += dt;
      particle.x += particle.vx * dt;
      particle.y += particle.vy * dt;
      return particle.age < particle.life;
    });
    this.pulses = this.pulses.filter((pulse) => {
      pulse.age += dt;
      return pulse.age < pulse.life;
    });
    if (this.labelDirty) {
      this.refreshLabels();
    }
    this.updateClusters();
  }

  refreshLabels() {
    const ordered = Array.from(this.agents.values())
      .filter((agent) => agent.role !== "queen")
      .sort((a, b) => a.id.localeCompare(b.id));
    const used = new Set();
    ordered.forEach((agent) => {
      const index = parseWorkerIndex(agent.id);
      if (index !== null) {
        agent.labelIndex = index;
        used.add(index);
      } else {
        agent.labelIndex = null;
      }
    });
    let nextIndex = 1;
    ordered.forEach((agent, idx) => {
      if (agent.labelIndex !== null) {
        return;
      }
      while (used.has(nextIndex)) {
        nextIndex += 1;
      }
      agent.labelIndex = nextIndex;
      used.add(nextIndex);
    });
    this.labelDirty = false;
  }

  updateClusters() {
    for (const cluster of this.clusters.values()) {
      let sumX = 0;
      let sumY = 0;
      let count = 0;
      for (const id of cluster.members) {
        const agent = this.agents.get(id);
        if (!agent) {
          continue;
        }
        const pos = this.positionForAgent(agent);
        sumX += pos.x;
        sumY += pos.y;
        count += 1;
      }
      if (count === 0) {
        continue;
      }
      cluster.center.x = sumX / count;
      cluster.center.y = sumY / count;
      cluster.radius = clamp(
        this.style.clusterRadiusBase + count * this.style.clusterRadiusStep,
        this.style.clusterRadiusMin,
        this.style.clusterRadiusMax,
      );
    }
  }
}

export { clusterKeyFromNamespace };
