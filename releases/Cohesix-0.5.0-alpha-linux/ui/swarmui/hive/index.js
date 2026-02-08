import { applyHiveEventsRange } from "../events.js";
import { readHiveTokens } from "./tokens.js";
import { buildHiveStyle } from "./style.js";
import { HiveWorld } from "./world.js";
import { HiveRenderer } from "./renderer.js";

const defaultConfig = {
  frame_cap_fps: 60,
  frame_cap_fps_degraded: 30,
  step_ms: 16,
  max_steps_per_frame: 3,
  lod_zoom_out: 0.7,
  lod_zoom_in: 1.25,
  lod_event_budget: 512,
  pending_event_cap: 4096,
  degrade_pressure: 1.0,
};

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export const createHiveController = (container, status, options = {}) => {
  const tokens = readHiveTokens();
  const style = buildHiveStyle(tokens);
  let world = new HiveWorld(style);
  const metrics = {
    frames: 0,
    renders: 0,
    pending: 0,
    dropped: 0,
    lastRenderAt: 0,
    lastFrameAt: 0,
  };
  const onAgentSelect = options.onAgentSelect;
  const selectAgent = (agentId) => {
    renderer.setSelectedAgent(agentId);
    if (onAgentSelect) {
      onAgentSelect(agentId);
    }
  };
  let renderer = new HiveRenderer(
    container,
    tokens,
    style,
    (clusterId) => {
      world.toggleCluster(clusterId);
    },
    (agentId) => {
      selectAgent(agentId);
    },
  );
  let config = { ...defaultConfig };
  let pending = [];
  let pendingHead = 0;
  let pendingSize = 0;
  let pendingCap = 0;
  let pressure = 0;
  let running = false;
  let lastFrame = 0;
  let lastRender = 0;
  let accumulator = 0;
  let lastPollMode = "detail";
  let lastQualityMode = "detail";

  const updateStatus = (text) => {
    if (status) {
      status.textContent = text;
    }
  };

  const resetPending = () => {
    pendingCap = Math.max(1, config.pending_event_cap || 0);
    pending = new Array(pendingCap);
    pendingHead = 0;
    pendingSize = 0;
    metrics.pending = 0;
    metrics.dropped = 0;
  };

  const enqueueEvent = (event) => {
    if (!pendingCap) {
      return;
    }
    if (pendingSize === pendingCap) {
      pendingHead = (pendingHead + 1) % pendingCap;
      pendingSize -= 1;
      metrics.dropped += 1;
    }
    const idx = (pendingHead + pendingSize) % pendingCap;
    pending[idx] = event;
    pendingSize += 1;
  };

  const enqueueBatch = (events) => {
    for (const event of events) {
      enqueueEvent(event);
    }
  };

  const computeLod = () => {
    const zoom = renderer.view.zoom;
    if (pressure >= config.degrade_pressure) {
      return "degraded";
    }
    if (zoom < config.lod_zoom_out) {
      return "cluster";
    }
    if (zoom > config.lod_zoom_in) {
      return "detail";
    }
    return "balanced";
  };

  const step = (time) => {
    if (!running) {
      return;
    }
    const delta = clamp((time - lastFrame) / 1000, 0, 0.25);
    lastFrame = time;
    metrics.frames += 1;
    metrics.lastFrameAt = time;
    accumulator += delta;
    const stepSeconds = config.step_ms / 1000;
    const lodMode = computeLod();
    if (lodMode !== lastQualityMode) {
      renderer.setQuality(lodMode);
      const particleScale = lodMode === "degraded" ? 0.4 : 1;
      world.setBudgets(particleScale, particleScale);
      lastQualityMode = lodMode;
    }
    const targetFps = pressure >= config.degrade_pressure
      ? Math.min(config.frame_cap_fps, config.frame_cap_fps_degraded)
      : config.frame_cap_fps;
    const frameInterval = 1000 / targetFps;
    let steps = 0;
    while (accumulator >= stepSeconds) {
      accumulator -= stepSeconds;
      steps += 1;
      const budget = config.lod_event_budget;
      const count = Math.min(budget, pendingSize);
      if (count > 0) {
        applyHiveEventsRange(world, pending, pendingHead, count, pendingCap, {
          pressure,
          spawnParticles: lodMode === "detail" && pressure < config.degrade_pressure,
        });
        pendingHead = (pendingHead + count) % pendingCap;
        pendingSize -= count;
      }
      world.update(stepSeconds);
      if (steps >= config.max_steps_per_frame) {
        accumulator = 0;
        break;
      }
    }
    let didRender = false;
    if (time - lastRender >= frameInterval) {
      renderer.render(world, lodMode);
      metrics.renders += 1;
      metrics.lastRenderAt = time;
      lastRender = time;
      didRender = true;
      if (lodMode !== lastPollMode) {
        updateStatus(`Hive ${lodMode}`);
        lastPollMode = lodMode;
      }
    }
    metrics.pending = pendingSize;
    if (didRender) {
      renderer.draw();
    }
    requestAnimationFrame(step);
  };

  const reset = () => {
    pressure = 0;
    world = new HiveWorld(style);
    resetPending();
    renderer.resetView();
    renderer.setSelectedAgent(null);
    renderer.setQuality("detail");
    world.setBudgets(1, 1);
    lastQualityMode = "detail";
  };

  if (typeof window !== "undefined") {
    window.__SWARMUI_HIVE_DEBUG = {
      getAgentScreenPositions: () => renderer.getAgentScreenPositions(),
      getAgentStates: () => renderer.getAgentStates(),
      getAgentLabels: () => renderer.getAgentLabels(),
      getMetrics: () => ({ ...metrics }),
      selectAgent: (agentId) => selectAgent(agentId),
    };
  }

  return {
    bootstrap: (bootstrap) => {
      config = { ...config, ...bootstrap.hive };
      reset();
      for (const agent of bootstrap.agents) {
        world.ensureAgent(agent.id, agent.namespace, agent.role);
      }
      world.ensureAgent("queen", "/queen", "queen");
      updateStatus(bootstrap.replay ? "Hive replay" : "Hive live");
    },
    ingest: (batch) => {
      pressure = batch.pressure ?? 0;
      if (batch.events && batch.events.length) {
        enqueueBatch(batch.events);
      }
      if (batch.done) {
        updateStatus("Hive replay complete");
      }
    },
    start: () => {
      if (running) {
        return;
      }
      running = true;
      lastFrame = performance.now();
      lastRender = 0;
      requestAnimationFrame(step);
    },
    stop: () => {
      running = false;
      resetPending();
    },
    resetView: () => renderer.resetView(),
    selectAgent: (agentId) => selectAgent(agentId),
    destroy: () => renderer.destroy(),
  };
};
