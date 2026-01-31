const numberVars = {
  "--hive-agent-size": 10,
  "--hive-agent-glow": 26,
  "--hive-pollen-size": 4,
  "--hive-pollen-life": 1.6,
  "--hive-pulse-size": 28,
  "--hive-pulse-life": 1.2,
  "--hive-flow-width": 1.4,
  "--hive-drift": 10,
  "--hive-heat-decay": 0.22,
  "--hive-flow-decay": 0.65,
};

const cssNumber = (value, fallback) => {
  if (!value) {
    return fallback;
  }
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const cssColor = (value, fallback) => (value && value.trim().length ? value.trim() : fallback);

const readCssVar = (root, name, fallback) => {
  const value = getComputedStyle(root).getPropertyValue(name);
  return value && value.trim().length ? value.trim() : fallback;
};

export const readHiveTokens = (root = document.documentElement) => {
  const numeric = {};
  for (const [name, fallback] of Object.entries(numberVars)) {
    numeric[name] = cssNumber(readCssVar(root, name), fallback);
  }
  return {
    fonts: {
      ui: readCssVar(root, "--font-ui", "Inter, sans-serif"),
    },
    sizes: {
      agent: numeric["--hive-agent-size"],
      glow: numeric["--hive-agent-glow"],
      pollen: numeric["--hive-pollen-size"],
      pulse: numeric["--hive-pulse-size"],
      flow: numeric["--hive-flow-width"],
      drift: numeric["--hive-drift"],
    },
    motion: {
      pollenLife: numeric["--hive-pollen-life"],
      pulseLife: numeric["--hive-pulse-life"],
      heatDecay: numeric["--hive-heat-decay"],
      flowDecay: numeric["--hive-flow-decay"],
    },
    colors: {
      agent: cssColor(readCssVar(root, "--color-hive-agent"), "#90b4ff"),
      agentGlow: cssColor(readCssVar(root, "--color-hive-agent-glow"), "#5b8cff"),
      pollen: cssColor(readCssVar(root, "--color-hive-pollen"), "#f5a352"),
      error: cssColor(readCssVar(root, "--color-hive-error"), "#ff6b6b"),
      heat: cssColor(readCssVar(root, "--color-hive-heat"), "#53e3c2"),
      cluster: cssColor(readCssVar(root, "--color-hive-cluster"), "#55607a"),
      flow: cssColor(readCssVar(root, "--color-hive-flow"), "#6dd7c2"),
      label: cssColor(readCssVar(root, "--color-ink-soft"), "#b7bcc9"),
    },
  };
};

export const hexToNumber = (value) => {
  const trimmed = value.replace("#", "");
  const hex = trimmed.length === 3
    ? trimmed
        .split("")
        .map((ch) => ch + ch)
        .join("")
    : trimmed;
  const parsed = Number.parseInt(hex, 16);
  return Number.isFinite(parsed) ? parsed : 0xffffff;
};
