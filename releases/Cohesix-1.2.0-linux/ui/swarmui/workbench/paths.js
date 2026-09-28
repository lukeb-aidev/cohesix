// Author: Lukas Bower
// Purpose: Select local artifacts with native file dialogs instead of requiring filesystem command knowledge.
// Copyright 2026 Lukas Bower
import { invoke, element, notice } from "./state.js";
export function attachPicker(input, kind = "file") {
  const row = element("div", undefined, "path-picker");
  input.replaceWith(row);
  row.append(input);
  const button = element("button", "Choose…");
  button.type = "button";
  button.setAttribute(
    "aria-label",
    `Choose ${input.getAttribute("aria-label") || input.id.replaceAll("-", " ")}`,
  );
  row.append(button);
  button.addEventListener("click", async () => {
    const response = await invoke("swarmui_choose_path", { kind });
    if (!response.ok) {
      notice(response.error, true);
      return;
    }
    if (response.result) {
      input.value = response.result;
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }
  });
}
export function initializePaths() {
  for (const [id, kind] of [
    ["story-path", "file"],
    ["evidence-path", "folder"],
    ["replay-path", "file"],
    ["tool-directory", "folder"],
  ]) {
    const input = document.getElementById(id);
    if (input) attachPicker(input, kind);
  }
}
