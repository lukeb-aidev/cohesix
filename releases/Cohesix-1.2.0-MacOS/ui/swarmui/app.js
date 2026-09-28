// Author: Lukas Bower
// Purpose: Compose the workbench modules around the real Rust-owned session and command bridge.
// Copyright 2026 Lukas Bower
import { hydrateIcons } from "./components/icon.js";
import { setupConsole } from "./components/console.js";
import { invoke, state, notice } from "./workbench/state.js";
import { initializeNavigation, initializeHelp } from "./workbench/navigation.js";
import { initializeSession, reflectSession } from "./workbench/session.js";
import { initializeOperations } from "./workbench/operations.js";
import { initializeMlx } from "./workbench/mlx.js";
import { initializeNamespace } from "./workbench/namespace.js";
import { initializeArtifacts } from "./workbench/artifacts.js";
import { initializeControls } from "./workbench/controls.js";
import { initializePaths } from "./workbench/paths.js";
import { resumeHive } from "./workbench/hive.js";
initializePaths();
initializeNavigation();
initializeHelp();
initializeSession();
initializeArtifacts();

hydrateIcons();
setupConsole(invoke);
const info = await invoke("swarmui_workbench_info");
initializeOperations(info.ok ? info.result.catalog : null);
initializeMlx(info.ok && info.result.local_mlx_host === true);
initializeControls(info.ok ? info.result.controls : []);
initializeNamespace(info.ok ? info.result.roots : []);
if (info.ok) {
  reflectSession(info.result);
  document.getElementById("tool-directory").value =
    info.result.tool_directory || "";
  if (info.result.mode?.hive_replay && state.view === "hive")
    await resumeHive();
} else notice(info.error);
window.addEventListener("session-connected", () => {
  if (state.view === "hive") resumeHive();
});

window.addEventListener("console-command-complete", async () => {
  const info = await invoke("swarmui_workbench_info");
  if (info.ok) reflectSession(info.result);
});
