const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export const applyHiveEventsRange = (world, events, start, count, capacity, options) => {
  const pressure = options?.pressure ?? 0;
  const spawnParticles = options?.spawnParticles ?? true;
  const intensity = clamp(1 - pressure * 0.4, 0.35, 1);
  if (!events || count <= 0) {
    return;
  }
  const size = capacity || events.length;
  for (let i = 0; i < count; i += 1) {
    const event = events[(start + i) % size];
    if (!event) {
      continue;
    }
    const agent = world.ensureAgent(event.agent, event.namespace, event.role);
    if (event.kind === "error") {
      world.emitError(agent);
    } else {
      world.emitTelemetry(agent, intensity, spawnParticles);
    }
  }
};

export const applyHiveEvents = (world, events, options) => {
  const size = events ? events.length : 0;
  applyHiveEventsRange(world, events, 0, size, size, options);
};
