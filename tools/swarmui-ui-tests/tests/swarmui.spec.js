const fs = require("fs");
const http = require("http");
const path = require("path");
const { test, expect } = require("@playwright/test");

const repoRoot = path.resolve(__dirname, "..", "..", "..");
const defaultReleaseDir = path.join(
  repoRoot,
  "releases",
  "Cohesix-0.3.0-alpha2-MacOS"
);
const releaseDir = process.env.SWARMUI_RELEASE_DIR
  ? path.resolve(process.env.SWARMUI_RELEASE_DIR)
  : defaultReleaseDir;
const uiRoot = path.join(releaseDir, "ui", "swarmui");

const helpLinesPath = path.join(__dirname, "fixtures", "help-lines.json");
const helpLines = JSON.parse(fs.readFileSync(helpLinesPath, "utf8"));

const hiveBootstrap = {
  replay: true,
  hive: {
    frame_cap_fps: 60,
    step_ms: 16,
    lod_zoom_out: 0.7,
    lod_zoom_in: 1.25,
    lod_event_budget: 512
  },
  agents: [
    {
      id: "worker-heart-1",
      namespace: "/worker/worker-heart-1",
      role: "worker-heartbeat"
    },
    {
      id: "worker-gpu-1",
      namespace: "/worker/worker-gpu-1",
      role: "worker-gpu"
    },
    {
      id: "worker-lora-1",
      namespace: "/worker/worker-lora-1",
      role: "worker-lora"
    },
    {
      id: "worker-bus-1",
      namespace: "/worker/worker-bus-1",
      role: "worker-bus"
    }
  ]
};

const hiveBatch = {
  pressure: 0,
  backlog: 0,
  dropped: 0,
  root: { reachable: true, cut_reason: null },
  sessions: { active: 1, draining: 0 },
  pressure_counters: { busy: 0, quota: 0, cut: 0, policy: 0 },
  events: [
    {
      kind: "telemetry",
      agent: "worker-heart-1",
      namespace: "/worker/worker-heart-1",
      role: "worker-heartbeat",
      reason: null
    },
    {
      kind: "telemetry",
      agent: "worker-gpu-1",
      namespace: "/worker/worker-gpu-1",
      role: "worker-gpu",
      reason: null
    }
  ],
  overlays: [
    {
      agent: "worker-heart-1",
      lines: ["tick 1", "tick 2"]
    },
    {
      agent: "worker-gpu-1",
      lines: ["gpu ok", "lease ok"]
    }
  ],
  detail: null,
  done: false
};

const ensureReleaseBundle = () => {
  const indexPath = path.join(uiRoot, "index.html");
  if (!fs.existsSync(indexPath)) {
    throw new Error(
      `SwarmUI release UI not found at ${indexPath}. Set SWARMUI_RELEASE_DIR to the latest release bundle path.`
    );
  }
};

const mimeTypeFor = (filePath) => {
  const ext = path.extname(filePath).toLowerCase();
  switch (ext) {
    case ".html":
      return "text/html";
    case ".js":
      return "application/javascript";
    case ".css":
      return "text/css";
    case ".svg":
      return "image/svg+xml";
    case ".json":
      return "application/json";
    case ".png":
      return "image/png";
    case ".jpg":
    case ".jpeg":
      return "image/jpeg";
    case ".woff2":
      return "font/woff2";
    default:
      return "application/octet-stream";
  }
};

const startStaticServer = () =>
  new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const urlPath = decodeURIComponent((req.url || "/").split("?")[0]);
      const safePath = urlPath === "/" ? "/index.html" : urlPath;
      const filePath = path.join(uiRoot, safePath);
      if (!filePath.startsWith(uiRoot)) {
        res.writeHead(403);
        res.end("forbidden");
        return;
      }
      fs.readFile(filePath, (err, data) => {
        if (err) {
          res.writeHead(404);
          res.end("not found");
          return;
        }
        res.writeHead(200, { "Content-Type": mimeTypeFor(filePath) });
        res.end(data);
      });
    });
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      resolve({ server, baseUrl: `http://127.0.0.1:${port}` });
    });
  });

const installTauriMock = async (page) => {
  await page.addInitScript(
    ({ helpLines, hiveBootstrap, hiveBatch }) => {
      const respond = async (cmd, payload) => {
        switch (cmd) {
          case "swarmui_mode":
            return { trace_replay: true, hive_replay: true };
          case "swarmui_hive_bootstrap":
            return hiveBootstrap;
          case "swarmui_hive_poll":
            return {
              ...hiveBatch,
              detail: payload?.detail_agent
                ? {
                    agent: payload.detail_agent,
                    lines: [`detail for ${payload.detail_agent}`, "line 2"]
                  }
                : null
            };
          case "swarmui_console_command":
            return { lines: helpLines };
          case "swarmui_connect":
            return { lines: ["OK CONNECT", "END"] };
          case "swarmui_tail_telemetry":
            return { lines: ["OK TAIL", "END"] };
          case "swarmui_fleet_snapshot":
            return { lines: ["OK FLEET", "END"] };
          case "swarmui_list_namespace":
            return { lines: ["OK LS", "END"] };
          case "swarmui_hive_reset":
            return { ok: true };
          case "swarmui_offline":
            return { ok: true };
          case "swarmui_mint_ticket":
            return "ticket-placeholder";
          default:
            throw new Error(`Unhandled invoke: ${cmd}`);
        }
      };

      window.__TAURI__ = {
        invoke: async (cmd, payload) => respond(cmd, payload)
      };
    },
    { helpLines, hiveBootstrap, hiveBatch }
  );
};

let serverHandle = null;
let baseUrl = null;

test.beforeAll(async () => {
  ensureReleaseBundle();
  const { server, baseUrl: url } = await startStaticServer();
  serverHandle = server;
  baseUrl = url;
});

test.afterAll(async () => {
  if (!serverHandle) {
    return;
  }
  await new Promise((resolve) => serverHandle.close(resolve));
});

test.beforeEach(async ({ page }) => {
  await installTauriMock(page);
  await page.goto(`${baseUrl}/index.html`, { waitUntil: "load" });
});

test("SwarmUI launches without error", async ({ page }) => {
  await expect(page).toHaveTitle(/SwarmUI/);
  await expect(page.locator("header.cohesix-banner")).toBeVisible();
  await expect(page.locator("#hive-status")).not.toContainText("failed");
});

test("Hive canvas renders in replay mode", async ({ page }) => {
  await expect(page.locator("#hive-status")).toContainText("Hive");
  await expect(page.locator("#hive-status")).not.toContainText("idle");
  const canvas = page.locator("#hive-canvas canvas");
  await expect(canvas).toHaveCount(1);
});

test("Live Hive labels enumerate workers", async ({ page }) => {
  await page.waitForTimeout(200);
  const labels = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getAgentLabels()
  );
  const visible = labels.filter((label) => label.visible);
  expect(visible.length).toBeGreaterThan(0);
  visible.forEach((label) => {
    expect(label.text).toMatch(/^\d+$/);
  });
});

test("Live Hive dots are clickable", async ({ page }) => {
  await page.waitForTimeout(200);
  const positions = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getAgentScreenPositions()
  );
  const target = positions.find((item) => item.id === "worker-gpu-1");
  expect(target).toBeTruthy();
  await page.mouse.click(target.x, target.y);
  await expect(page.locator("#hive-detail-title")).toContainText("worker-gpu-1");
});

test("Live Hive performance harness stays responsive", async ({ page }) => {
  await page.waitForTimeout(600);
  const metrics = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics()
  );
  expect(metrics.renders).toBeGreaterThan(10);
  expect(metrics.pending).toBeLessThan(1024);
});

test("Embedded coh prompt accepts input", async ({ page }) => {
  const input = page.locator("#console-input");
  await input.fill("help");
  await input.press("Enter");
  await expect(page.locator("#console-output")).toContainText("coh> help");
});

test("Help command emits expected transcript lines", async ({ page }) => {
  const input = page.locator("#console-input");
  await input.fill("help");
  await input.press("Enter");

  const output = page.locator("#console-output");
  await expect(output).toContainText("SwarmUI console commands:");

  const expected = ["coh> help", ...helpLines];
  await expect.poll(async () => {
    const lines = await page.$$eval("#console-output .console-line", (nodes) =>
      nodes.map((node) => node.textContent || "")
    );
    return lines;
  }).toEqual(expected);
});

test("Replay header snapshot matches baseline", async ({ page }) => {
  const banner = page.locator("header.cohesix-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toHaveScreenshot("swarmui-banner.png");
});
