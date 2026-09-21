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
  ensureUiRootExists,
} = require("../swarmui-paths.cjs");

const uiRoot = resolveUiRoot();

const helpLinesPath = path.join(__dirname, "fixtures", "help-lines.json");
const helpLines = JSON.parse(fs.readFileSync(helpLinesPath, "utf8"));
const referenceStories = Object.fromEntries(
  ["lora", "recovery"].map((name) => [
    name,
    JSON.parse(
      fs.readFileSync(
        path.join(__dirname, "fixtures", `${name}-story.json`),
        "utf8",
      ),
    ),
  ]),
);

const hiveBootstrap = {
  replay: true,
  hive: {
    frame_cap_fps: 60,
    step_ms: 16,
    lod_zoom_out: 0.7,
    lod_zoom_in: 1.25,
    lod_event_budget: 512,
    status_poll_ms: 400,
  },
  namespace_roots: ["/proc", "/queen", "/shard", "/worker", "/log", "/gpu"],
  agents: [
    {
      id: "opaque-heart-a",
      namespace: "/shard/a4/worker/opaque-heart-a/telemetry",
      role: "worker-heartbeat",
      worker: { declaration: "executable", lifecycle: "ready" },
    },
    {
      id: "opaque-gpu-b",
      namespace: "/shard/01/worker/opaque-gpu-b/telemetry",
      role: "worker-gpu",
      worker: { declaration: "executable", lifecycle: "starting" },
    },
    {
      id: "opaque-lora-c",
      namespace: "/shard/02/worker/opaque-lora-c/telemetry",
      role: "worker-lora",
      worker: { declaration: "executable", lifecycle: "ready" },
    },
    {
      id: "opaque-bus-d",
      namespace: "/shard/e3/worker/opaque-bus-d/telemetry",
      role: "worker-bus",
      worker: { declaration: "model-only" },
    },
    {
      id: "worker-gpu-looking",
      namespace: "/shard/ff/worker/worker-gpu-looking/telemetry",
      role: "worker",
      worker: {},
    },
  ],
};

const hiveBatch = {
  agents: [
    {
      id: "opaque-gpu-b",
      namespace: "/shard/01/worker/opaque-gpu-b/telemetry",
      role: "worker-gpu",
      worker: {
        declaration: "executable",
        lifecycle: "ready",
        artifact: "verified",
        receipt: "confirmed",
        execution_proof: "qemu",
      },
    },
  ],
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
        seq: 42,
      },
      {
        id: "sched-2",
        role: "worker-heartbeat",
        priority: 2,
        ticks: 1,
        budget_ms: 40,
        seq: 43,
      },
    ],
  },
  lease: {
    summary: {
      active: 1,
      preemptions: 1,
      quotas: 2,
      max_active: 8,
      max_preemptions: 16,
    },
    active: [
      {
        id: "lease-1",
        subject: "queen",
        resource: "gpu0",
        ttl_s: 300,
        priority: 5,
        state: "active",
        seq: 9,
      },
    ],
    preemptions: [
      {
        id: "lease-0",
        subject: "opaque-gpu-b",
        resource: "gpu1",
        reason: "timeout",
        seq: 7,
      },
    ],
  },
  events: [
    {
      kind: "telemetry",
      agent: "opaque-heart-a",
      namespace: "/shard/a4/worker/opaque-heart-a/telemetry",
      role: "worker-heartbeat",
      reason: null,
    },
    {
      kind: "telemetry",
      agent: "opaque-gpu-b",
      namespace: "/shard/01/worker/opaque-gpu-b/telemetry",
      role: "worker-gpu",
      reason: null,
    },
  ],
  overlays: [
    {
      agent: "opaque-heart-a",
      lines: ["tick 1", "tick 2"],
    },
    {
      agent: "opaque-gpu-b",
      lines: ["gpu ok", "lease ok"],
    },
  ],
  detail: null,
  done: false,
};

const ensureUiRoot = () => {
  if (!ensureUiRootExists(uiRoot)) {
    throw new Error(
      `SwarmUI UI root not found at ${uiRoot}. Set SWARMUI_UI_ROOT (source UI) or SWARMUI_RELEASE_DIR (release bundle).`,
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
    ...(options.mode || {}),
  };
  await page.addInitScript(
    ({ helpLines, hiveBootstrap, hiveBatch, mode, referenceStories }) => {
      const pollCalls = [];
      window.__SWARMUI_TEST = {
        hivePollCalls: pollCalls,
        invokeCalls: [],
        detailLines: null,
        omitDetail: false,
        overlays: hiveBatch.overlays,
      };
      const respond = async (cmd, payload) => {
        const state = window.__SWARMUI_TEST;
        state.invokeCalls.push({ cmd, payload });
        switch (cmd) {
          case "swarmui_reference":
            state.connection = null;
            mode.offline = true;
            mode.trace_replay = true;
            return referenceStories[payload.name];
          case "swarmui_choose_path":
            return "/chosen/artifact.json";
          case "swarmui_workbench_info":
            return {
              mode,
              fixture: true,
              connection: state.connection || null,
              roots: hiveBootstrap.namespace_roots,
              tool_directory: "/installed/bin",
              catalog: {
                name: "coh",
                fields: [],
                commands: [
                  {
                    name: "plan",
                    help: "Plan a governed workflow",
                    fields: [
                      {
                        id: "workflow",
                        required: true,
                        help: "Registered workflow",
                      },
                      {
                        id: "recipe",
                        flag: true,
                        help: "Compose a CUDA recipe",
                      },
                    ],
                    commands: [],
                  },
                  {
                    name: "evidence",
                    fields: [],
                    commands: [
                      {
                        name: "story",
                        help: "Verify independently signed outcomes",
                        fields: [
                          {
                            id: "input",
                            required: true,
                            help: "Signed causal graph",
                          },
                          {
                            id: "trust",
                            required: true,
                            help: "Independent trust policy",
                          },
                          { id: "cas", required: true, help: "Artifact store" },
                        ],
                        commands: [],
                      },
                    ],
                  },
                ],
              },
            };
          case "swarmui_session_open":
            if (payload.request.credential === "invalid")
              return { ok: false, lines: ["ERR AUTH refused", "END"] };
            state.connection = {
              endpoint: payload.request.endpoint,
              transport: payload.request.transport,
              role: payload.request.role,
            };
            return { ok: true, lines: ["OK ATTACH role=queen", "END"] };
          case "swarmui_session_close":
            state.connection = null;
            return null;
          case "swarmui_namespace":
            if (payload.path === "/proc/boot")
              return payload.verb === "ls"
                ? { ok: false, lines: ["ERR LS reason=policy detail=invalid-path", "END"] }
                : { ok: true, lines: ["OK CAT", "boot_identity=fixture", "END"] };
            if (payload.path === "/proc/denied")
              return { ok: false, lines: ["ERR AUTH forbidden", "END"] };
            return {
              ok: true,
              lines: [
                `OK ${payload.verb.toUpperCase()}`,
                "boot",
                "root",
                "END",
              ],
            };
          case "swarmui_host_preview":
            return {
              review_id: "fixture-review",
              argv: [
                "coh",
                ...payload.request.operation,
                "--input=/evidence/graph.json",
              ],
            };
          case "swarmui_host_run":
            return {
              success: true,
              outcome: "command_completed",
              stdout: '{"outcome":"verified"}',
              stderr: "",
            };
          case "swarmui_mode":
            return mode;
          case "swarmui_hive_bootstrap":
            return hiveBootstrap;
          case "swarmui_hive_poll":
            pollCalls.push(Date.now());
            return {
              ...hiveBatch,
              overlays: state.overlays,
              // Tauri commands use camelCase argument names by default.
              detail:
                payload?.detailAgent && !state.omitDetail
                  ? {
                      agent: payload.detailAgent,
                      lines: state.detailLines || [
                        `detail for ${payload.detailAgent}`,
                        "line 2",
                      ],
                    }
                  : null,
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
          invoke: async (cmd, payload) => respond(cmd, payload),
        },
      };
    },
    { helpLines, hiveBootstrap, hiveBatch, mode, referenceStories },
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
    { timeout: 3000 },
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
  await page.locator('[data-view="console"]').click();
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
  await expect(page.locator("header.appbar")).toBeVisible();
  await expect(page.locator("#hive-status")).not.toContainText("failed");
});

test("Tauri core invoke bridge powers namespace actions", async ({ page }) => {
  await page.locator('[data-view="namespaces"]').click();
  await page.locator("#namespace-go").click();
  await expect(page.locator("#namespace-output")).toContainText("OK LS");
  await expect(page.locator("#namespace-output")).not.toContainText(
    "Tauri API unavailable",
  );
});

test("Namespace file navigation clears stale entries and preserves refusal boundaries", async ({ page }) => {
  await page.locator('[data-view="namespaces"]').click();
  await page.locator("#namespace-path").fill("/proc");
  await page.locator("#namespace-go").click();
  await expect(page.locator("#namespace-entries button")).toHaveCount(2);
  await page.locator("#namespace-entries button").first().click();
  await expect(page.locator("#namespace-output")).toContainText("boot_identity=fixture");
  await expect(page.locator("#namespace-entries button")).toHaveCount(0);
  await expect(page.locator("#namespace-kind")).toHaveText("File · scoped read");
  await page.locator("#namespace-path").fill("/proc/denied");
  await page.locator("#namespace-go").click();
  await expect(page.locator("#namespace-output")).toContainText("ERR AUTH forbidden");
  await page.locator("#namespace-transcript").click();
  await expect(page.locator("#transcript-drawer")).not.toContainText("cat /proc/denied");
});

test("Namespace direct reads and refusals identify their own response path", async ({ page }) => {
  await page.locator('[data-view="namespaces"]').click();
  await page.locator("#namespace-path").fill("/shard");
  await page.locator("#namespace-go").click();
  await expect(page.locator("#namespace-title")).toHaveText("/shard");
  for (const [path, action, output, crumbs] of [
    ["/proc/boot", "read", "boot_identity=fixture", ["proc /", "boot /"]],
    ["/log/queen.log", "tail", "OK TAIL", ["log /", "queen.log /"]],
    ["/proc/denied", "read", "ERR AUTH forbidden", ["proc /", "denied /"]],
  ]) {
    await page.locator("#namespace-path").fill(path);
    await page.locator(`#namespace-${action}`).click();
    await expect(page.locator("#namespace-output")).toContainText(output);
    await expect(page.locator("#namespace-title")).toHaveText(path);
    await expect(page.locator("#namespace-breadcrumbs button")).toHaveText(crumbs);
    await expect(page.locator("#namespace-entries button")).toHaveCount(0);
  }
  await expect(page.locator("#namespace-kind")).toHaveText(
    "Unavailable or refused · inspect exact reason",
  );
});

test("Concise contextual help is keyboard accessible, bounded and dismissible", async ({ page }) => {
  const help = page.getByRole("button", { name: "Help with this screen" });
  const tooltip = page.getByRole("tooltip");
  await help.focus();
  await expect(tooltip).toBeVisible();
  await expect(tooltip).toContainText("Connect to your controller");
  await expect(help).toHaveAttribute("aria-describedby", "workbench-tooltip");
  await page.keyboard.press("Escape");
  await expect(tooltip).toBeHidden();
  await page.getByRole("button", { name: "Replay", exact: true }).click();
  await help.click();
  await expect(tooltip).toContainText("This closes your live connection");
  const box = await tooltip.boundingBox();
  const viewport = page.viewportSize();
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  await page.locator('[data-view="settings"]').click();
  await expect(tooltip).toBeHidden();
  await page.locator("#hive-snapshot-key").focus();
  await expect(tooltip).toContainText("not a password");
  await page.keyboard.press("Tab");
  await expect(tooltip).toBeHidden();
  await page.locator("#transcript-toggle").hover();
  await expect(tooltip).toContainText("its exact reply");
  await page.keyboard.press("Escape");
  await expect(tooltip).toBeHidden();
});

test("Spectrum shell controls are mounted", async ({ page }) => {
  await expect(page.locator("#session-role")).toHaveJSProperty(
    "tagName",
    "SELECT",
  );
  await expect(page.locator("#session-ticket")).toHaveJSProperty(
    "tagName",
    "INPUT",
  );
  await expect(page.locator("#connect")).toHaveJSProperty(
    "tagName",
    "SP-BUTTON",
  );
  await expect(page.locator("#console-send")).toHaveJSProperty(
    "tagName",
    "SP-BUTTON",
  );

  const themeState = await page
    .locator("sp-theme.app-theme")
    .evaluate((node) => ({
      sheetCount: node.shadowRoot?.adoptedStyleSheets?.length ?? 0,
      token: getComputedStyle(node)
        .getPropertyValue("--spectrum-neutral-content-color-default")
        .trim(),
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
    "Canvas label density is only gated on the desktop visual target.",
  );
  await focusHiveCanvas(page);
  const labels = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getAgentLabels(),
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
    { timeout: 3000 },
  );
  const calls = await page.evaluate(() =>
    window.__SWARMUI_TEST.hivePollCalls.slice(0, 2),
  );
  expect(calls.length).toBe(2);
  expect(calls[1] - calls[0]).toBeGreaterThanOrEqual(350);
});

test("Live Hive keeps rendering during scroll", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Scroll-driven render cadence is only gated on the desktop visual target.",
  );
  await page.waitForTimeout(200);
  const before = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics(),
  );
  await page.evaluate(() => window.scrollTo(0, 200));
  await page.waitForTimeout(120);
  const mid = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics(),
  );
  await page.waitForTimeout(240);
  const after = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics(),
  );
  expect(after.frames).toBeGreaterThan(before.frames);
  expect(after.renders).toBeGreaterThanOrEqual(mid.renders);
});

test("Live Hive selection wiring activates the detail pane", async ({
  page,
}) => {
  await focusHiveCanvas(page);
  await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.selectAgent("opaque-gpu-b"),
  );
  await expect(page.locator("#hive-detail-title")).toContainText(
    "opaque-gpu-b",
  );
  await expect(page.locator("#hive-detail-state")).toContainText(
    "declaration=executable",
  );
  await expect(page.locator("#hive-detail-state")).toContainText(
    "lifecycle=ready",
  );
  await expect(page.locator("#hive-detail-state")).toContainText(
    "receipt=confirmed",
  );
  await expect(page.locator("#hive-detail-state")).toContainText(
    "artifact=verified",
  );
  await expect(page.locator("#hive-detail-state")).toContainText("proof=qemu");
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "detail for opaque-gpu-b\nline 2",
  );
  await page.evaluate(() => {
    window.__SWARMUI_TEST.detailLines = ["updated GPU telemetry", "sequence 2"];
  });
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "updated GPU telemetry\nsequence 2",
  );
});

test("Live detail keeps updating below the offscreen canvas", async ({
  page,
}, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Native Mac reading layout.",
  );
  await page.setViewportSize({ width: 820, height: 600 });
  await focusHiveCanvas(page);
  await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.selectAgent("opaque-gpu-b"),
  );
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "detail for opaque-gpu-b\nline 2",
  );
  await page
    .locator("#hive-detail-title")
    .evaluate((node) => node.scrollIntoView({ block: "start" }));
  await expect(page.locator("#hive-canvas")).not.toBeInViewport();
  await expect(page.locator("#hive-detail-lines")).toBeInViewport();
  await page.evaluate(() => {
    window.__SWARMUI_TEST.detailLines = ["new telemetry while reading details"];
  });
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "new telemetry while reading details",
  );
});

test("Live Hive snapshot key uses the native command argument", async ({
  page,
}) => {
  await page.locator('[data-view="settings"]').click();
  await page.locator("#hive-snapshot-key").fill("operator-snapshot");
  await page.locator('[data-view="hive"]').click();
  await page.locator("#hive-start").click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__SWARMUI_TEST.invokeCalls
            .filter((call) => call.cmd === "swarmui_hive_bootstrap")
            .at(-1)?.payload,
      ),
    )
    .toEqual({ role: "queen", ticket: null, snapshotKey: "operator-snapshot" });
});

test("Selected overlay fallback updates when detail is unavailable", async ({
  page,
}) => {
  await page.evaluate(() => {
    window.__SWARMUI_TEST.omitDetail = true;
    window.__SWARMUI_TEST.overlays = [
      { agent: "opaque-gpu-b", lines: ["GPU telemetry sequence 1"] },
    ];
  });
  await focusHiveCanvas(page);
  await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.selectAgent("opaque-gpu-b"),
  );
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "GPU telemetry sequence 1",
  );
  await page.evaluate(() => {
    window.__SWARMUI_TEST.overlays = [
      { agent: "opaque-gpu-b", lines: ["GPU telemetry sequence 2"] },
    ];
  });
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "GPU telemetry sequence 2",
  );
  await page.evaluate(() => {
    window.__SWARMUI_TEST.overlays = [];
  });
  await expect(page.locator("#hive-detail-lines")).toHaveText(
    "No telemetry yet.",
  );
});

test("Role-looking Worker ids do not synthesize structured state", async ({
  page,
}) => {
  await focusHiveCanvas(page);
  await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.selectAgent("worker-gpu-looking"),
  );
  await expect(page.locator("#hive-detail-title")).toContainText(
    "worker-gpu-looking",
  );
  await expect(page.locator("#hive-detail-state")).toHaveText(
    "declaration=unknown lifecycle=unknown receipt=unknown artifact=unknown proof=unknown",
  );
});

test("Scheduler and lease panels render /proc data", async ({ page }) => {
  await page.waitForTimeout(200);
  await expect(
    page.locator(".hive-schedule > .hive-schedule__section"),
  ).toHaveCount(2);
  await expect(page.locator("#hive-schedule-summary")).toContainText(
    "Queue 2/64",
  );
  await expect(page.locator("#hive-schedule-queue")).toContainText("sched-1");
  await expect(page.locator("#hive-schedule-queue")).toContainText("sched-2");
  const scheduleOverflow = await page
    .locator("#hive-schedule-queue")
    .evaluate((node) => node.scrollWidth - node.clientWidth);
  expect(scheduleOverflow).toBeLessThanOrEqual(1);
  await expect(page.locator("#hive-lease-summary")).toContainText("Active 1/8");
  await expect(page.locator("#hive-lease-active")).toContainText("lease-1");
  await expect(page.locator("#hive-lease-preemptions")).toContainText(
    "lease-0",
  );
});

test("Responsive workbench keeps navigation and content within the viewport", async ({
  page,
}) => {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow).toBeLessThanOrEqual(1);
  await expect(page.locator(".dock")).toBeVisible();
  await expect(page.locator(".appbar")).toBeVisible();
  await page.locator('[data-view="operations"]').click();
  await expect(page.locator("#operation-search")).toBeVisible();
});

test("Live Hive overlays remain interactive under load", async ({ page }) => {
  await page.waitForTimeout(200);
  const cards = page.locator("#hive-overlays .hive-telemetry__card");
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(1)).toHaveAttribute("aria-pressed", "false");
  await cards.nth(1).click();
  await expect(cards.nth(1)).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#hive-detail-title")).toContainText(
    "opaque-gpu-b",
  );
});

test("Live Hive performance harness stays responsive", async ({ page }) => {
  await focusHiveCanvas(page);
  await page.waitForFunction(
    () => window.__SWARMUI_HIVE_DEBUG?.getMetrics?.().renders >= 5,
    null,
    { timeout: 3000 },
  );
  const metrics = await page.evaluate(() =>
    window.__SWARMUI_HIVE_DEBUG.getMetrics(),
  );
  expect(metrics.renders).toBeGreaterThanOrEqual(5);
  expect(metrics.pending).toBeLessThan(1024);
});

test("Embedded coh prompt accepts input", async ({ page }) => {
  await page.locator('[data-view="console"]').click();
  const editor = page.locator("#console-input input");
  await expect(editor).toHaveAttribute("autocorrect", "off");
  await expect(editor).toHaveAttribute("autocapitalize", "off");
  await expect(editor).toHaveAttribute("spellcheck", "false");
  await runConsoleCommand(page, "help");
  await expect(page.locator("#console-output")).toContainText("coh> help");
});

test("Offline snapshot replay disables the console prompt", async ({
  page,
}) => {
  await installTauriMock(page, {
    mode: { trace_replay: false, hive_replay: true, offline: true },
  });
  await page.goto(`${baseUrl}/index.html`, { waitUntil: "load" });

  await expect(page.locator(".console-panel")).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.locator("#console-send")).toBeDisabled();
  await expect(page.locator("#console-input")).toHaveAttribute("disabled", "");
  await expect(page.locator("#console-output")).toContainText(
    "Console unavailable in offline snapshot replay mode.",
  );
});

test("Transcript panels preserve line breaks", async ({ page }) => {
  await page.locator('[data-view="namespaces"]').click();
  await page.locator("#namespace-go").click();
  await expect(page.locator("#namespace-output")).toContainText("OK LS");
  expect(
    await page
      .locator("#namespace-output")
      .evaluate((n) => getComputedStyle(n).whiteSpace),
  ).toBe("pre-wrap");
  expect(await page.locator("#namespace-output").textContent()).toContain("\n");
});

test("Help command emits expected transcript lines", async ({ page }) => {
  await runConsoleCommand(page, "help");

  const output = page.locator("#console-output");
  await expect(output).toContainText("SwarmUI console commands:");

  const expected = ["coh> help", ...helpLines];
  await expect
    .poll(async () => {
      const lines = await page.$$eval(
        "#console-output .console-line",
        (nodes) => nodes.map((node) => node.textContent || ""),
      );
      return lines;
    })
    .toEqual(expected);
});

test("Mint ticket populates the session ticket field", async ({ page }) => {
  await page.locator('[data-view="tickets"]').click();
  await page.getByText("Mint a capability ticket", { exact: true }).click();
  await fillField(page, "#session-subject", "opaque-gpu-b");
  await page.locator("#mint-ticket").click();
  await expect(page.locator("#mint-status")).toContainText("Ticket minted");
  await expect
    .poll(async () => readFieldValue(page, "#session-ticket"))
    .toBe("ticket-placeholder");
});

test("Replay header snapshot matches baseline", async ({ page }, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-desktop",
    "Visual desktop baseline is anchored on the WebKit desktop project.",
  );
  const banner = page.locator("header.appbar");
  await expect(banner).toBeVisible();
  // Spectrum inherits the native system font stack on each supported host.
  await expect(banner).toHaveScreenshot(`swarmui-banner-${process.platform}.png`);
});

test("Responsive topbar snapshot matches baseline", async ({
  page,
}, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-narrow",
    "Responsive visual baseline is anchored on the WebKit narrow project.",
  );
  const topbar = page.locator("header.appbar");
  await expect(topbar).toBeVisible();
  await expect(topbar).toHaveScreenshot("swarmui-topbar-narrow.png");
});

test("Responsive scheduler snapshot matches baseline", async ({
  page,
}, testInfo) => {
  test.skip(
    testInfo.project.name !== "webkit-narrow",
    "Responsive visual baseline is anchored on the WebKit narrow project.",
  );
  await page.waitForTimeout(200);
  const scheduler = page.locator(".hive-schedule-panel");
  await expect(scheduler).toBeVisible();
  await expect(scheduler).toHaveScreenshot("swarmui-schedule-narrow.png");
});

test("workbench connection refuses invalid auth and saves no secrets", async ({
  page,
}) => {
  await page.locator("#connection-open").click();
  await page.locator("#connection-endpoint").fill("http://127.0.0.1:8080");
  await page.locator("#connection-credential").fill("invalid");
  await page.locator("#connect").click();
  await expect(page.locator("#connection-result")).toContainText("ERR AUTH");
  await expect(page.locator("#connection-dialog")).toBeVisible();
  await page.locator("#connection-credential").fill("private-test-credential");
  await page.locator("#session-ticket").fill("private-test-ticket");
  await page.locator("#connection-name").fill("My hive");
  await page.locator("#connect").click();
  await expect(page.locator("#connection-dialog")).not.toBeVisible();
  const stored = await page.evaluate(() => JSON.stringify(localStorage));
  expect(stored).toContain("My hive");
  expect(stored).not.toContain("private-test");
  await expect(page.locator("#connection-credential")).toHaveValue("");
});

test("workbench guided host action requires review and retains exact result", async ({
  page,
}) => {
  await page
    .locator('[data-host-operation="evidence story"]')
    .evaluate((n) => n.click());
  await page.locator("#op-input").fill("/evidence/graph.json");
  await page.locator("#op-trust").fill("/trust/keys.json");
  await page.locator("#op-cas").fill("/cas");
  await page.getByText("Review action", { exact: true }).first().click();
  await expect(page.locator("#review-dialog")).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        window.__SWARMUI_TEST.invokeCalls.filter(
          (c) => c.cmd === "swarmui_host_run",
        ).length,
    ),
  ).toBe(0);
  await page.locator("#review-confirm").click();
  await expect(page.locator("#operation-output")).toContainText(
    "command_completed",
  );
  await expect(page.locator("#operation-output")).toContainText(
    '"outcome":"verified"',
  );
});

test("live-hive-performance inactive desks stop polling", async ({ page }) => {
  await expect
    .poll(() => page.evaluate(() => window.__SWARMUI_TEST.hivePollCalls.length))
    .toBeGreaterThan(0);
  await page.getByRole("button", { name: "Evidence", exact: true }).click();
  const count = await page.evaluate(
    () => window.__SWARMUI_TEST.hivePollCalls.length,
  );
  await page.waitForTimeout(900);
  expect(
    await page.evaluate(() => window.__SWARMUI_TEST.hivePollCalls.length),
  ).toBe(count);
});

test("workbench keyboard palette navigates and restores focus", async ({
  page,
}) => {
  await page.locator("#palette-open").click();
  await page.locator("#palette-search").fill("namespace");
  await page.locator("#palette-search").press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#palette-dialog")).not.toBeVisible();
  await expect(page.locator('[data-desk="namespaces"]')).toBeVisible();
  await expect(page.locator("#source-mode")).toHaveText("FIXTURE");
});

test("live-ai-showcase keeps historical causal owners and failed recovery separate", async ({
  page,
}) => {
  await page.locator('.dock [data-view="replay"]').click();
  await page.locator('[data-reference="recovery"]').click();
  await expect(page.locator("#source-mode")).toHaveText("FIXTURE");
  await expect(page.locator(".story-outcome")).toHaveText("recovered failure");
  await expect(page.locator(".story-canvas canvas")).toBeVisible();
  await expect(page.locator(".ownership-legend button")).toHaveCount(7);
  await expect(page.locator(".evidence-ribbon button")).toHaveCount(9);
  await page
    .getByRole("button", { name: "rollback succeeded", exact: false })
    .click();
  await expect(page.locator(".story-inspector")).toContainText(
    "Observation in verified CAS",
  );
  await expect(page.locator(".story-inspector")).toContainText("worker-lora");
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await expect(page.locator("#presentation-mode")).toBeInViewport();
  await expect(page.locator("#presentation-mode")).toHaveText("FIXTURE");
  await page.locator("#showcase-toggle").click();
  await expect(page.locator("#presentation-mode")).toBeVisible();
  await expect(page.locator("#presentation-mode")).toHaveText("FIXTURE");
  await page.keyboard.press("Escape");
  await expect(page.locator(".dock")).toBeVisible();
});

test("live-ai-showcase reduced motion freezes motion while preserving incoming observations", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.reload();
  await expect
    .poll(() =>
      page.evaluate(
        () => window.__SWARMUI_HIVE_DEBUG?.getState().reducedMotion,
      ),
    )
    .toBe(true);
  await page.locator('.dock [data-view="replay"]').click();
  await page.locator('[data-reference="lora"]').click();
  await page
    .getByRole("button", { name: "Play walkthrough", exact: true })
    .click();
  await expect(page.locator(".story-player span")).toHaveText("2 / 7");
  await page.waitForTimeout(1900);
  await expect(page.locator(".story-player span")).toHaveText("2 / 7");
});

test("workbench native artifact chooser fills the selected path", async ({
  page,
}) => {
  await page.locator('.dock [data-view="story"]').click();
  await page.getByRole("button", { name: "Choose Run report path" }).click();
  await expect(page.locator("#story-path")).toHaveValue(
    "/chosen/artifact.json",
  );
});
