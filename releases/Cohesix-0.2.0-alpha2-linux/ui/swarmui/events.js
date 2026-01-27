const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export const applyHiveEvents = (world, events, options) => {
  const pressure = options?.pressure ?? 0;
  const spawnParticles = options?.spawnParticles ?? true;
  const intensity = clamp(1 - pressure * 0.4, 0.35, 1);
  for (const event of events) {
    const agent = world.ensureAgent(event.agent, event.namespace);
    if (event.kind === "error") {
      world.emitError(agent);
    } else {
      world.emitTelemetry(agent, intensity, spawnParticles);
    }
  }
};
