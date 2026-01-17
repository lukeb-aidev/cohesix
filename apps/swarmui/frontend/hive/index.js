import { applyHiveEvents } from "../events.js";
import { readHiveTokens } from "./tokens.js";
import { buildHiveStyle } from "./style.js";
import { HiveWorld } from "./world.js";
import { HiveRenderer } from "./renderer.js";

const defaultConfig = {
  frame_cap_fps: 60,
  step_ms: 16,
  lod_zoom_out: 0.7,
  lod_zoom_in: 1.25,
  lod_event_budget: 512,
};

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export const createHiveController = (container, status) => {
  const tokens = readHiveTokens();
  const style = buildHiveStyle(tokens);
  let world = new HiveWorld(style);
  let renderer = new HiveRenderer(container, tokens, style, (clusterId) => {
    world.toggleCluster(clusterId);
  });
  let config = { ...defaultConfig };
  let pending = [];
  let pressure = 0;
  let running = false;
  let lastFrame = 0;
  let lastRender = 0;
  let accumulator = 0;
  let lastPollMode = "detail";

  const updateStatus = (text) => {
    if (status) {
      status.textContent = text;
    }
  };

  const computeLod = () => {
    const zoom = renderer.view.zoom;
    if (pressure > 1) {
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
    accumulator += delta;
    const stepSeconds = config.step_ms / 1000;
    const lodMode = computeLod();
    const frameInterval = 1000 / config.frame_cap_fps;
    while (accumulator >= stepSeconds) {
      accumulator -= stepSeconds;
      const budget = config.lod_event_budget;
      const batch = pending.splice(0, budget);
      if (batch.length) {
        applyHiveEvents(world, batch, {
          pressure,
          spawnParticles: lodMode === "detail" && pressure < 1,
        });
      }
      world.update(stepSeconds);
    }
    if (time - lastRender >= frameInterval) {
      renderer.render(world, lodMode);
      lastRender = time;
      if (lodMode !== lastPollMode) {
        updateStatus(`Hive ${lodMode}`);
        lastPollMode = lodMode;
      }
    }
    requestAnimationFrame(step);
  };

  const reset = () => {
    pending = [];
    pressure = 0;
    world = new HiveWorld(style);
    renderer.resetView();
  };

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
        pending.push(...batch.events);
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
      lastRender = lastFrame;
      requestAnimationFrame(step);
    },
    stop: () => {
      running = false;
      pending = [];
    },
    resetView: () => renderer.resetView(),
    destroy: () => renderer.destroy(),
  };
};
