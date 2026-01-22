import { hexToNumber } from "./tokens.js";

export const buildHivePalette = (tokens) => ({
  agent: hexToNumber(tokens.colors.agent),
  agentGlow: hexToNumber(tokens.colors.agentGlow),
  pollen: hexToNumber(tokens.colors.pollen),
  error: hexToNumber(tokens.colors.error),
  heat: hexToNumber(tokens.colors.heat),
  cluster: hexToNumber(tokens.colors.cluster),
  flow: hexToNumber(tokens.colors.flow),
  label: hexToNumber(tokens.colors.label),
});
