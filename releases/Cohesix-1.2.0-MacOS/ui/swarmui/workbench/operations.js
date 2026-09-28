// Author: Lukas Bower
// Purpose: Render canonical host-tool schemas as guided forms with explicit action review.
// Copyright 2026 Lukas Bower
import {
  state,
  invoke,
  element,
  notice,
  transcript,
  sanitize,
} from "./state.js";
import { attachPicker } from "./paths.js";
import { navigate } from "./navigation.js";
const title = (text) =>
  text
    .replaceAll("_", " ")
    .replaceAll("-", " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
const common = {
  plan: "Plan a workflow",
  apply: "Admit the next stage",
  watch: "Observe a run",
  verify: "Verify a run",
  recover: "Recover or cancel a run",
  explain: "Explain a workflow",
  "peft release": "Release a private adapter",
  "gpu workload": "Manage a GPU workload",
  doctor: "Check your environment",
  providers: "Explore registered providers",
};
let operations = [],
  selected = null,
  pending = null;
export function catalogOperations(schema) {
  const result = [];
  const visit = (node, path, parents) => {
    const fields = [
      ...new Map(
        [...parents, ...(node.fields || [])].map((f) => [f.id, f]),
      ).values(),
    ];
    if (!node.commands?.length && path.length)
      result.push({
        ...node,
        path,
        fields,
        label: common[path.join(" ")] || title(path.join(" · ")),
      });
    else
      (node.commands || []).forEach((child) =>
        visit(child, [...path, child.name], fields),
      );
  };
  visit(schema, [], []);
  return result;
}
function inputField(field, values) {
  const container = element("div");
  const id = `op-${field.id}`;
  const label = element(
    "label",
    `${title(field.id)}${field.required ? " *" : ""}`,
  );
  label.htmlFor = id;
  let control;
  if (field.flag) {
    control = element("input");
    control.type = "checkbox";
    control.checked = values?.[0] === "true";
    label.className = "checkbox-row";
    control.id = id;
    control.dataset.field = field.id;
    label.prepend(control);
    container.append(label);
  } else {
    if (field.choices?.length) {
      control = element("select");
      if (!field.required) control.add(new Option("Use default", ""));
      field.choices.forEach((c) => control.add(new Option(title(c), c)));
    } else if (field.multiple) {
      control = element("textarea");
      control.placeholder = "One value per line";
    } else {
      control = element("input");
      control.type = "text";
    }
    control.id = id;
    control.dataset.field = field.id;
    control.required = field.required;
    control.value = values?.join("\n") || "";
    if (!control.value && field.choices?.length && field.required)
      control.value = field.choices[0];
    if (field.default?.length)
      control.placeholder = `Default: ${field.default.join(", ")}`;
    container.append(label, control);
    if (field.path)
      attachPicker(
        control,
        field.value_names?.includes("DIR") || /^(cas|root|state_dir|journal|registry)$/.test(field.id)
          ? "folder" : /^(out|output|report)$/.test(field.id) ? "save" : "file",
      );
  }
  const help = element(
    "p",
    field.help ||
      (field.multiple
        ? "Enter each value on its own line."
        : "Validated by the installed command."),
    "field-help",
  );
  help.id = `${id}-help`;
  control.setAttribute("aria-describedby", help.id);
  container.append(help);
  return container;
}
export function selectOperation(path, defaults = {}) {
  const key = Array.isArray(path) ? path.join(" ") : path;
  const operation = operations.find((o) => o.path.join(" ") === key);
  if (!operation) {
    notice(
      "The installed command catalog is unavailable. Open the native application and select a matching installation in Settings.",
      true,
    );
    return;
  }
  selected = operation;
  navigate("operations");
  const editor = document.getElementById("operation-editor");
  editor.replaceChildren();
  editor.append(
    element("span", "HOST TOOL / COH", "section-kicker"),
    element("h2", operation.label),
    element("p", operation.help),
  );
  if (operation.unavailable) {
    editor.append(element("p", operation.unavailable, "read-output"));
    return;
  }
  const form = element("form");
  form.id = "operation-form";
  const grid = element("div", undefined, "form-grid");
  operation.fields.forEach((field) =>
    grid.append(inputField(field, defaults[field.id])),
  );
  form.append(grid);
  const actions = element("div", undefined, "button-row");
  const review = element("sp-button", "Review action");
  review.variant = "accent";
  review.type = "button";
  actions.append(review);
  form.append(actions);
  editor.append(form);
  const output = element(
    "pre",
    "Results will appear here. Command completion and verified outcome are distinct.",
    "read-output",
  );
  output.id = "operation-output";
  output.setAttribute("role", "status");
  editor.append(output);
  const reviewAction = async (e) => {
    e.preventDefault();
    if (!form.reportValidity() || state.hostBusy) return;
    const values = {};
    for (const field of operation.fields) {
      const input = document.getElementById(`op-${field.id}`);
      if (field.flag) {
        if (input.checked) values[field.id] = ["true"];
      } else if (input.value.trim()) {
        values[field.id] = field.multiple
          ? input.value
              .split("\n")
              .map((s) => s.trim())
              .filter(Boolean)
          : [input.value.trim()];
      }
    }
    const request = { operation: operation.path, values };
    const response = await invoke("swarmui_host_preview", { request });
    if (!response.ok) {
      output.textContent = response.error;
      notice(response.error, true);
      return;
    }
    pending = {
      request,
      reviewId: response.result.review_id,
      label: operation.label,
    };
    document.getElementById("review-description").textContent = operation.help;
    document.getElementById("review-preview").textContent = response.result.argv
      .map((arg) => JSON.stringify(arg))
      .join(" ");
    document.getElementById("review-dialog").showModal();
  };
  form.addEventListener("submit", reviewAction);
  review.addEventListener("click", reviewAction);
  document
    .querySelectorAll("#operation-list button")
    .forEach((n) =>
      n.classList.toggle("selected", n.dataset.operation === key),
    );
}
export function initializeOperations(schema) {
  window.addEventListener("select-operation", (event) =>
    selectOperation(event.detail),
  );
  state.catalog = schema;
  operations = schema ? catalogOperations(schema) : [];
  window.dispatchEvent(
    new CustomEvent("operation-catalog", {
      detail: operations.map((o) => ({
        name: o.label,
        help: o.help,
        path: o.path,
      })),
    }),
  );
  const list = document.getElementById("operation-list"),
    search = document.getElementById("operation-search");
  const render = () => {
    list.replaceChildren();
    let previous = "";
    for (const operation of operations.filter((o) =>
      `${o.label} ${o.help} ${o.path.join(" ")}`
        .toLowerCase()
        .includes(search.value.toLowerCase()),
    )) {
      const group =
        operation.path.length > 1
          ? title(operation.path[0])
          : "Workflow & host";
      if (previous !== group) {
        list.append(element("p", group, "operation-group"));
        previous = group;
      }
      const button = element("button", operation.label);
      button.dataset.operation = operation.path.join(" ");
      button.addEventListener("click", () => selectOperation(operation.path));
      list.append(button);
    }
    if (!operations.length)
      list.append(
        element(
          "p",
          "Open the native app to load its operation catalog.",
          "empty-copy",
        ),
      );
  };
  search.addEventListener("input", render);
  render();
  document
    .querySelectorAll("[data-host-operation]")
    .forEach((n) =>
      n.addEventListener("click", () =>
        selectOperation(n.dataset.hostOperation),
      ),
    );
  document.querySelectorAll("[data-action]").forEach((n) =>
    n.addEventListener("click", () =>
      n.dataset.action === "cuda"
        ? selectOperation("plan", {
            workflow: ["cuda-reference"],
            recipe: ["true"],
          })
        : selectOperation("peft release", { mode: ["plan"] }),
    ),
  );
  document
    .getElementById("review-cancel")
    .addEventListener("click", () =>
      document.getElementById("review-dialog").close(),
    );
  document
    .getElementById("review-confirm")
    .addEventListener("click", async () => {
      if (!pending || state.hostBusy) return;
      const operation = pending;
      pending = null;
      state.hostBusy = true;
      document.getElementById("review-dialog").close();
      const output = document.getElementById("operation-output");
      if (output)
        output.textContent =
          "Running the installed command. Keep the original operation identity for recovery.";
      notice(
        `${operation.label} is running. You can keep browsing; a second host operation waits for this one.`,
      );
      const response = await invoke("swarmui_host_run", {
        request: operation.request,
        reviewId: operation.reviewId,
      });
      state.hostBusy = false;
      const result = response.ok ? response.result : null;
      const text = result
        ? `${result.outcome}\n\n${result.stdout}${result.stderr ? `\n${result.stderr}` : ""}`
        : response.error;
      if (output?.isConnected) output.textContent = sanitize(text);
      transcript(operation.label, text);
      notice(
        result?.success
          ? "Command completed. Inspect its result and evidence for the outcome."
          : text.slice(0, 500),
        !result?.success,
      );
      window.dispatchEvent(
        new CustomEvent("host-result", {
          detail: { operation: operation.request.operation, result },
        }),
      );
    });
}
