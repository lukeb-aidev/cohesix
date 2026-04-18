// Author: Lukas Bower
// Purpose: Define the SwarmUI Playwright browser matrix and snapshot policy.
// Copyright 2026 Lukas Bower

const { resolveReleaseDir } = require("./swarmui-paths.cjs");

const releaseDir = resolveReleaseDir();

module.exports = {
  testDir: require("path").join(__dirname, "tests"),
  timeout: 30000,
  expect: {
    timeout: 10000,
    toHaveScreenshot: {
      maxDiffPixelRatio: 0.01
    }
  },
  use: {
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
  projects: [
    {
      name: "webkit-desktop",
      use: {
        browserName: "webkit",
        viewport: { width: 1400, height: 900 }
      }
    },
    {
      name: "webkit-narrow",
      use: {
        browserName: "webkit",
        viewport: { width: 820, height: 1180 }
      }
    },
    {
      name: "chromium-tablet",
      use: {
        browserName: "chromium",
        viewport: { width: 1180, height: 820 }
      }
    }
  ]
};
