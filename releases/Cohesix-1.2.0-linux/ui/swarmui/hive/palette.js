import { hexToNumber } from "./tokens.js";

export const buildHivePalette = (tokens) => ({
  agent: hexToNumber(tokens.colors.agent),
  agentGlow: hexToNumber(tokens.colors.agentGlow),
  pollen: hexToNumber(tokens.colors.pollen),
  error: hexToNumber(tokens.colors.error),
  heat: hexToNumber(tokens.colors.heat),
  cluster: hexToNumber(tokens.colors.cluster),
  flow: hexToNumber(tokens.colors.flow),
  queen: hexToNumber(tokens.colors.queen),
  worker: hexToNumber(tokens.colors.worker),
  workerHeart: hexToNumber(tokens.colors.workerHeart),
  workerGpu: hexToNumber(tokens.colors.workerGpu),
  workerLora: hexToNumber(tokens.colors.workerLora),
  workerBus: hexToNumber(tokens.colors.workerBus),
  label: hexToNumber(tokens.colors.label),
});
