// Author: Lukas Bower
// Purpose: Offer structured Queen controls whose previews and payloads are owned by Rust.
// Copyright 2026 Lukas Bower
import { invoke, element, transcript, notice } from "./state.js";
const controls = [
  {
    action: "spawn",
    name: "Start a Worker",
    fields: [
      ["role", "Worker role", ["heartbeat", "gpu", "lora"]],
      ["ticks", "Heartbeat ticks"],
      ["gpu_id", "GPU identifier"],
      ["mem_mb", "GPU memory (MiB)"],
      ["streams", "GPU streams"],
      ["ttl_s", "Lease or budget duration (seconds)"],
      ["ops", "Operation budget"],
      ["priority", "GPU priority"],
      ["budget_ttl_s", "GPU budget duration"],
      ["budget_ops", "GPU operation budget"],
    ],
  },
  {
    action: "kill",
    name: "Stop a Worker",
    fields: [["id", "Worker identifier"]],
  },
  {
    action: "budget",
    name: "Set a budget",
    fields: [
      ["ttl_s", "Duration (seconds)"],
      ["ops", "Operation count"],
    ],
  },
  {
    action: "append",
    name: "Append to a control file",
    fields: [
      ["path", "Absolute namespace path"],
      ["payload", "One-line payload"],
    ],
  },
  { action: "ping", name: "Check session liveness", fields: [] },
  { action: "log", name: "Read Queen activity", fields: [] },
  { action: "bootinfo", name: "Inspect target boot", fields: [] },
  {
    action: "caps",
    name: "Inspect capability authority",
    fields: [["mode", "Diagnostic view", ["", "mcs"]]],
  },
  {
    action: "smp",
    name: "Inspect CPU and scheduling",
    fields: [
      ["mode", "Diagnostic view", ["activity", "mcs", "poll-time", "dump"]],
    ],
  },
  { action: "mem", name: "Inspect target memory", fields: [] },
  {
    action: "cachelog",
    name: "Inspect bounded cache diagnostics",
    fields: [["count", "Maximum rows"]],
  },
  { action: "netstats", name: "Inspect network counters", fields: [] },
  { action: "nettest", name: "Run admitted network diagnostic", fields: [] },
  { action: "test", name: "Run admitted target diagnostic", fields: [] },
  { action: "reboot", name: "Restart the target", fields: [] },
];
export function initializeControls(structured = []) {
  const root = document.getElementById("control-actions");
  for (const control of [...controls, ...structured]) {
    const details = element("details"),
      summary = element("summary", control.name),
      form = element("form");
    details.append(summary, form);
    if (control.enabled === false) {
      form.append(
        element(
          "p",
          "This control is disabled by the selected profile.",
          "field-help",
        ),
      );
      root.append(details);
      continue;
    }
    if (control.path)
      form.append(
        element(
          "p",
          `Target: ${control.path}. The target validates policy, bounds and state transitions.`,
          "field-help",
        ),
      );
    for (const [key, label, choices] of control.fields) {
      const labelNode = element("label", label),
        input = element(choices ? "select" : "input");
      input.id = `control-${control.action}-${key}`;
      labelNode.htmlFor = input.id;
      input.dataset.field = key;
      if (choices)
        choices.forEach((value) => input.add(new Option(value, value)));
      else input.type = "text";
      form.append(labelNode, input);
      if (control.action === "spawn" && key !== "role") {
        labelNode.dataset.spawnField = key;
        input.dataset.spawnField = key;
      }
    }
    if (control.action === "spawn") {
      const role = form.querySelector('[data-field="role"]');
      const update = () => {
        const visible =
          role.value === "heartbeat"
            ? ["ticks", "ttl_s", "ops"]
            : role.value === "gpu"
              ? [
                  "gpu_id",
                  "mem_mb",
                  "streams",
                  "ttl_s",
                  "priority",
                  "budget_ttl_s",
                  "budget_ops",
                ]
              : ["ttl_s", "ops"];
        form.querySelectorAll("[data-spawn-field]").forEach((node) => {
          node.hidden = !visible.includes(node.dataset.spawnField);
          if (node.dataset.field) node.disabled = node.hidden;
        });
      };
      role.addEventListener("change", update);
      update();
    }
    if (control.action === "reboot")
      form.append(
        element(
          "p",
          "This interrupts the target and disconnects active clients. Review the action and reconnect after boot.",
          "field-help",
        ),
      );
    const preview = element("sp-button", "Review action");
    preview.variant = "secondary";
    preview.type = "button";
    const output = element("pre", undefined, "read-output");
    output.hidden = true;
    const submit = element("sp-button", "Confirm & submit");
    submit.variant = "accent";
    submit.hidden = true;
    form.append(preview, output, submit);
    root.append(details);
    let pending = null;
    form.addEventListener("input", () => {
      pending = null;
      submit.hidden = true;
    });
    preview.addEventListener("click", async (e) => {
      e.preventDefault();
      const fields = {};
      form.querySelectorAll("[data-field]").forEach((n) => {
        if (!n.disabled && n.value.trim())
          fields[n.dataset.field] = n.value.trim();
      });
      const request = { action: control.action, fields };
      const result = await invoke("swarmui_control", {
        request,
        submit: false,
      });
      output.hidden = false;
      output.textContent = result.ok ? result.result.preview : result.error;
      pending = result.ok
        ? { request, reviewId: result.result.review_id }
        : null;
      submit.hidden = !pending;
    });
    submit.addEventListener("click", async (e) => {
      e.preventDefault();
      if (!pending) return;
      const { request, reviewId } = pending;
      pending = null;
      submit.hidden = true;
      const result = await invoke("swarmui_control", {
        request,
        submit: true,
        reviewId,
      });
      const text = result.ok ? result.result.lines.join("\n") : result.error;
      output.textContent = text;
      transcript(control.name, text);
      notice(
        result.ok && result.result.ok
          ? "Queen response received. Admission does not prove execution."
          : text,
        !(result.ok && result.result.ok),
      );
    });
  }
}
