// Author: Lukas Bower
// Purpose: Playwright coverage for SwarmUI UI workflows and Live Hive rendering.
// Copyright 2026 Lukas Bower

const fs = require("fs");
const http = require("http");
const path = require("path");
const { test, expect } = require("@playwright/test");
const {
  repoRoot,
  resolveUiRoot,
  ensureUiRootExists
} = require("../swarmui-paths.cjs");

const uiRoot = resolveUiRoot();

const helpLinesPath = path.join(__dirname, "fixtures", "help-lines.json");
const helpLines = JSON.parse(fs.readFileSync(helpLinesPath, "utf8"));

const hiveBootstrap = {
  replay: true,
  hive: {
    frame_cap_fps: 60,
    step_ms: 16,
    lod_zoom_out: 0.7,
    lod_zoom_in: 1.25,
    lod_event_budget: 512,
    status_poll_ms: 400
  },
  namespace_roots: ["/proc", "/queen", "/worker", "/log", "/gpu"],
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
  schedule: {
    summary: { queue: 2, dequeued: 7, dropped: 1, max_entries: 64 },
    queue: [
      {
        id: "sched-1",
        role: "worker-gpu",
        priority: 5,
        ticks: 3,
        budget_ms: 120,
        seq: 42
      },
      {
        id: "sched-2",
        role: "worker-heartbeat",
        priority: 2,
        ticks: 1,
        budget_ms: 40,
        seq: 43
      }
    ]
  },
  lease: {
    summary: { active: 1, preemptions: 1, quotas: 2, max_active: 8, max_preemptions: 16 },
    active: [
      {
        id: "lease-1",
        subject: "queen",
        resource: "gpu0",
        ttl_s: 300,
        priority: 5,
        state: "active",
        seq: 9
      }
    ],
    preemptions: [
      {
        id: "lease-0",
        subject: "worker-gpu-1",
        resource: "gpu1",
        reason: "timeout",
        seq: 7
      }
    ]
  },
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

const ensureUiRoot = () => {
  if (!ensureUiRootExists(uiRoot)) {
    throw new Error(
      `SwarmUI UI root not found at ${uiRoot}. Set SWARMUI_UI_ROOT (source UI) or SWARMUI_RELEASE_DIR (release bundle).`
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

const installTauriMock = async (page, options = {}) => {
  const mode = {
    trace_replay: true,
    hive_replay: true,
    offline: false,
    ...(options.mode || {})
  };
  await page.addInitScript(
    ({ helpLines, hiveBootstrap, hiveBatch, mode }) => {
      const pollCalls = [];
      window.__SWARMUI_TEST = { hivePollCalls: pollCalls };
      const respond = async (cmd, payload) => {
        switch (cmd) {
          case "swarmui_mode":
            return mode;
          case "swarmui_hive_bootstrap":
            return hiveBootstrap;
          case "swarmui_hive_poll":
            pollCalls.push(Date.now());
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
        core: {
          invoke: async (cmd, payload) => respond(cmd, payload)
        }
      };
    },
    { helpLines, hiveBootstrap, hiveBatch, mode }
  );
};

const focusHiveCanvas = async (page) => {
  const canvas = page.locator("#hive-canvas");
  await canvas.scrollIntoViewIfNeeded();
  await canvas.dispatchEvent("pointermove");
  await page.evaluate(() => window.__SWARMUI_HIVE_DEBUG?.forceFrame?.());
  await page.waitForFunction(
    () => window.__SWARMUI_HIVE_DEBUG?.getMetrics?.().renders > 0,
    null,
    { timeout: 3000 }
  );
};

const fillField = async (page, selector, value) => {
  const host = page.locator(selector);
  const tagName = await host.evaluate((node) => node.tagName.toLowerCase());
  if (tagName === "sp-textfield") {
    await host.locator("input").fill(value);
    return;
  }
  await host.fill(value);
};

const readFieldValue = async (page, selector) => {
  const host = page.locator(selector);
  return host.evaluate((node) => {
    const value = "value" in node ? node.value : "";
    return typeof value === "string" ? value : String(value || "");
  });
};

const runConsoleCommand = async (page, command) => {
  await fillField(page, "#console-input", command);
  await page.locator("#console-send").click();
};

let serverHandle = null;
let baseUrl = null;

test.beforeAll(async () => {
  ensureUiRoot();
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
  await expect(page.locator("sp-theme.app-theme")).toBeVisible();
  await expect(page.locator("header.cohesix-banner")).toBeVisible();
  await expect(page.locator("#hive-status")).not.toContainText("failed");
});

test("Tauri core invoke bridge powers namespace actions", async ({ page }) => {
  await page.locator("#load-namespace").click();
  await expect(page.locator("#namespace-output")).toContainText("OK LS");
  await expect(page.locator("#namespace-output")).not.toContainText("Tauri API unavailable");
});

test("Spectrum shell controls are mounted", async ({ page }) => {
  await expect(page.locator("#session-role")).toHaveJSProperty("tagName", "SP-PICKER");
  await expect(page.locator("#session-ticket")).toHaveJSProperty("tagName", "SP-TEXTFIELD");
  await expect(page.locator("#connect")).toHaveJSProperty("tagName", "SP-BUTTON");
  await expect(page.locator("#console-send")).toHaveJSProperty("tagName", "SP-BUTTON");

  const themeState = await page.locator("sp-theme.app-theme").evaluate((node) => ({
    sheetCount: node.shadowRoot?.adoptedStyleSheets?.length ?? 0,
    token: getComputedStyle(node)
      .getPropertyValue("--spectrum-neutral-content-color-default")
      .trim()
  }));
  expect(themeState.sheetCount).toBeGreaterThan(0);
  expect(themeState.token.length).toBeGreaterThan(0);
});

test("Hive canvas renders in replay mode", async ({ page }) => {
  await expect(page.locator("#hive-status")).toContainText("Hive");
  await expect(page.locator("#hive-status")).not.toContainText("idle");
  const canvas = page.locator("#hive-canvas canvas");
  await expect(canvas).toHaveCount(1);
});

test("Live Hive labels enumerate workers", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Canvas label density is only gated on the desktop visual target."
  );
  await focusHiveCanvas(page);
  const labels = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getAgentLabels()
  );
  const visible = labels.filter((label) => label.visible);
  expect(visible.length).toBeGreaterThan(0);
  visible.forEach((label) => {
    expect(label.text).toMatch(/^\d+$/);
  });
});

test("Live Hive poll interval honors status poll policy", async ({ page }) => {
  await page.waitForFunction(
    () => window.__SWARMUI_TEST?.hivePollCalls?.length >= 2,
    null,
    { timeout: 3000 }
  );
  const calls = await page.evaluate(() =>
    window.__SWARMUI_TEST.hivePollCalls.slice(0, 2)
  );
  expect(calls.length).toBe(2);
  expect(calls[1] - calls[0]).toBeGreaterThanOrEqual(350);
});

test("Live Hive keeps rendering during scroll", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Scroll-driven render cadence is only gated on the desktop visual target."
  );
  await page.waitForTimeout(200);
  const before = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics()
  );
  await page.evaluate(() => window.scrollTo(0, 200));
  await page.waitForTimeout(120);
  const mid = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics()
  );
  await page.waitForTimeout(240);
  const after = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics()
  );
  expect(after.frames).toBeGreaterThan(before.frames);
  expect(after.renders).toBeGreaterThanOrEqual(mid.renders);
});

test("Live Hive selection wiring activates the detail pane", async ({ page }) => {
  await focusHiveCanvas(page);
  await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.selectAgent("worker-gpu-1")
  );
  await expect(page.locator("#hive-detail-title")).toContainText("worker-gpu-1");
});

test("Scheduler and lease panels render /proc data", async ({ page }) => {
  await page.waitForTimeout(200);
  await expect(page.locator("#hive-schedule-summary")).toContainText(
    "Queue 2/64"
  );
  await expect(page.locator("#hive-schedule-queue")).toContainText("sched-1");
  await expect(page.locator("#hive-schedule-queue")).toContainText("sched-2");
  await expect(page.locator("#hive-lease-summary")).toContainText("Active 1/8");
  await expect(page.locator("#hive-lease-active")).toContainText("lease-1");
  await expect(page.locator("#hive-lease-preemptions")).toContainText("lease-0");
});

test("Responsive shell keeps the grid transitions intentional", async ({ page }) => {
  const width = page.viewportSize()?.width ?? 0;
  const layout = await page.evaluate(() => {
    const readTracks = (selector) =>
      getComputedStyle(document.querySelector(selector))
        .gridTemplateColumns
        .trim()
        .split(/\s+/)
        .filter(Boolean).length;

    return {
      topbar: readTracks(".topbar"),
      session: readTracks(".session-grid"),
      shell: readTracks(".layout"),
    };
  });

  if (width <= 900) {
    expect(layout.topbar).toBe(1);
    expect(layout.session).toBe(2);
    expect(layout.shell).toBe(1);
    await expect(page.locator(".hive-schedule__row.header")).toBeHidden();
    return;
  }

  if (width <= 1240) {
    expect(layout.topbar).toBe(1);
    expect(layout.session).toBe(2);
    expect(layout.shell).toBe(2);
    return;
  }

  expect(layout.topbar).toBe(2);
  expect(layout.session).toBe(3);
  expect(layout.shell).toBe(2);
});

test("Live Hive overlays remain interactive under load", async ({ page }) => {
  await page.waitForTimeout(200);
  const cards = page.locator("#hive-overlays .hive-telemetry__card");
  await expect(cards).toHaveCount(2);
  await cards.nth(1).click();
  await expect(page.locator("#hive-detail-title")).toContainText("worker-gpu-1");
});

test("Live Hive performance harness stays responsive", async ({ page }) => {
  await focusHiveCanvas(page);
  await page.waitForTimeout(500);
  const metrics = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics()
  );
  expect(metrics.renders).toBeGreaterThanOrEqual(5);
  expect(metrics.pending).toBeLessThan(1024);
});

test("Embedded coh prompt accepts input", async ({ page }) => {
  await runConsoleCommand(page, "help");
  await expect(page.locator("#console-output")).toContainText("coh> help");
});

test("Offline snapshot replay disables the console prompt", async ({ page }) => {
  await installTauriMock(page, {
    mode: { trace_replay: false, hive_replay: true, offline: true }
  });
  await page.goto(`${baseUrl}/index.html`, { waitUntil: "load" });

  await expect(page.locator(".console-panel")).toHaveAttribute(
    "aria-disabled",
    "true"
  );
  await expect(page.locator("#console-send")).toBeDisabled();
  await expect(page.locator("#console-input")).toHaveAttribute("disabled", "");
  await expect(page.locator("#console-output")).toContainText(
    "Console unavailable in offline snapshot replay mode."
  );
});

test("Transcript panels preserve line breaks", async ({ page }) => {
  await page.locator("#load-fleet").click();
  await expect(page.locator("#fleet-output")).toContainText("OK FLEET");
  const whiteSpace = await page.locator("#fleet-output").evaluate((node) =>
    getComputedStyle(node).whiteSpace
  );
  expect(whiteSpace).toBe("pre-wrap");
  const text = await page.locator("#fleet-output").textContent();
  expect(text).toContain("\n");
});

test("Help command emits expected transcript lines", async ({ page }) => {
  await runConsoleCommand(page, "help");

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

test("Mint ticket populates the session ticket field", async ({ page }) => {
  await fillField(page, "#session-subject", "worker-gpu-1");
  await page.locator("#mint-ticket").click();
  await expect(page.locator("#mint-status")).toContainText("Ticket minted");
  await expect.poll(async () => readFieldValue(page, "#session-ticket")).toBe(
    "ticket-placeholder"
  );
});

test("Replay header snapshot matches baseline", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Visual desktop baseline is anchored on the WebKit desktop project."
  );
  const banner = page.locator("header.cohesix-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toHaveScreenshot("swarmui-banner.png");
});

test("Responsive topbar snapshot matches baseline", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-narrow",
    "Responsive visual baseline is anchored on the WebKit narrow project."
  );
  const topbar = page.locator("header.topbar");
  await expect(topbar).toBeVisible();
  await expect(topbar).toHaveScreenshot("swarmui-topbar-narrow.png");
});

test("Responsive scheduler snapshot matches baseline", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-narrow",
    "Responsive visual baseline is anchored on the WebKit narrow project."
  );
  await page.waitForTimeout(200);
  const scheduler = page.locator(".hive-schedule-panel");
  await expect(scheduler).toBeVisible();
  await expect(scheduler).toHaveScreenshot("swarmui-schedule-narrow.png");
});
