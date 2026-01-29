const path = require("path");

const repoRoot = path.resolve(__dirname, "..", "..");
const defaultReleaseDir = path.join(
  repoRoot,
  "releases",
  "Cohesix-0.2.0-alpha2-MacOS"
);

const releaseDir = process.env.SWARMUI_RELEASE_DIR
  ? path.resolve(process.env.SWARMUI_RELEASE_DIR)
  : defaultReleaseDir;

module.exports = {
  testDir: path.join(__dirname, "tests"),
  timeout: 30000,
  expect: {
    timeout: 10000,
    toHaveScreenshot: {
      maxDiffPixelRatio: 0.01
    }
  },
  use: {
    browserName: "webkit",
    viewport: { width: 1400, height: 900 },
    deviceScaleFactor: 1,
    colorScheme: "light",
    locale: "en-US",
    screenshot: "only-on-failure"
  },
  reporter: [["list"]],
  metadata: {
    swarmuiReleaseDir: releaseDir
  },
  snapshotPathTemplate:
    "{testDir}/__screenshots__/{projectName}/{testFilePath}/{arg}{ext}",
  projects: [{ name: "webkit" }]
};
