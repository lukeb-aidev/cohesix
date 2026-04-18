// Author: Lukas Bower
// Purpose: Resolve the current SwarmUI UI root for Playwright coverage without drifting to stale release bundles.
// Copyright 2026 Lukas Bower

const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..", "..");
const sourceUiRoot = path.join(repoRoot, "apps", "swarmui", "frontend");

const resolveReleaseDir = () => {
  if (!process.env.SWARMUI_RELEASE_DIR) {
    return null;
  }
  return path.resolve(process.env.SWARMUI_RELEASE_DIR);
};

const resolveUiRoot = () => {
  if (process.env.SWARMUI_UI_ROOT) {
    return path.resolve(process.env.SWARMUI_UI_ROOT);
  }
  const releaseDir = resolveReleaseDir();
  if (releaseDir) {
    return path.join(releaseDir, "ui", "swarmui");
  }
  return sourceUiRoot;
};

const ensureUiRootExists = (uiRoot) => {
  const indexPath = path.join(uiRoot, "index.html");
  return fs.existsSync(indexPath);
};

module.exports = {
  repoRoot,
  sourceUiRoot,
  resolveReleaseDir,
  resolveUiRoot,
  ensureUiRootExists,
};
