// Author: Lukas Bower
// Purpose: Wire the embedded SwarmUI console prompt and transcript playback.
// Copyright 2026 Lukas Bower

const classifyLine = (line) => {
  if (line === "END") {
    return "end";
  }
  if (line.startsWith("OK ")) {
    return "ok";
  }
  if (line.startsWith("ERR ")) {
    return "err";
  }
  if (line.startsWith("coh>")) {
    return "cmd";
  }
  if (line.startsWith("  ")) {
    return "help";
  }
  return "info";
};

const createLineNode = (line) => {
  const node = document.createElement("div");
  node.classList.add("console-line");
  const kind = classifyLine(line);
  node.classList.add(`console-${kind}`);
  node.textContent = line;
  return node;
};

export const setupConsole = (invoke) => {
  const output = document.getElementById("console-output");
  const input = document.getElementById("console-input");
  const send = document.getElementById("console-send");
  const clear = document.getElementById("console-clear");
  const stop = document.getElementById("console-stop");
  const dumpLog = document.getElementById("console-dump-log");

  if (!output || !input || !send) {
    return;
  }

  let streamToken = 0;
  let streaming = false;
  let linesBuffered = 0;
  const maxLines = 600;

  if (stop) {
    stop.disabled = true;
  }

  const readValue = () => {
    if (!("value" in input)) {
      return "";
    }
    const raw = input.value ?? "";
    return typeof raw === "string" ? raw : String(raw);
  };

  const clearValue = () => {
    if (!("value" in input)) {
      return;
    }
    input.value = "";
    input.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
    input.dispatchEvent(new Event("change", { bubbles: true, composed: true }));
  };

  const resetPlaceholder = () => {
    if (!output.querySelector(".placeholder")) {
      return;
    }
    output.textContent = "";
  };

  const appendLine = (line) => {
    resetPlaceholder();
    output.appendChild(createLineNode(line));
    linesBuffered += 1;
    if (linesBuffered > maxLines) {
      const excess = linesBuffered - maxLines;
      for (let i = 0; i < excess; i += 1) {
        if (!output.firstChild) {
          break;
        }
        output.removeChild(output.firstChild);
        linesBuffered -= 1;
      }
    }
    output.scrollTop = output.scrollHeight;
  };

  const endStream = () => {
    streaming = false;
    if (stop) {
      stop.disabled = true;
    }
  };

  const streamLines = (lines) => {
    if (!Array.isArray(lines) || lines.length === 0) {
      endStream();
      return;
    }
    const token = streamToken;
    let index = 0;
    const step = () => {
      if (token !== streamToken) {
        endStream();
        return;
      }
      if (index >= lines.length) {
        endStream();
        return;
      }
      appendLine(lines[index]);
      index += 1;
      requestAnimationFrame(step);
    };
    step();
  };

  const runCommand = async (line) => {
    const trimmed = line.trim();
    if (!trimmed) {
      return;
    }
    appendLine(`coh> ${trimmed}`);
    if (stop) {
      stop.disabled = false;
    }
    streaming = true;
    streamToken += 1;
    const res = await invoke("swarmui_console_command", { line: trimmed });
    if (!res.ok) {
      appendLine(`ERR CONSOLE ${res.error}`);
      endStream();
      return;
    }
    streamLines(res.result?.lines || []);
  };

  const downloadText = (filename, text) => {
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = filename || "queen.log.txt";
    document.body.appendChild(link);
    link.click();
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  };

  const dumpQueenLog = async () => {
    if (dumpLog) {
      dumpLog.disabled = true;
    }
    const res = await invoke("swarmui_dump_queen_log");
    if (!res.ok) {
      appendLine(`ERR LOGDUMP ${res.error}`);
      if (dumpLog) {
        dumpLog.disabled = false;
      }
      return;
    }
    const dump = res.result || {};
    const text = typeof dump.text === "string" ? dump.text : "";
    const filename = typeof dump.filename === "string" ? dump.filename : "queen.log.txt";
    downloadText(filename, text);
    appendLine(
      `OK LOGDUMP file=${filename} lines=${dump.lines || 0} bytes=${dump.bytes || text.length}`,
    );
    if (dumpLog) {
      dumpLog.disabled = false;
    }
  };

  send.addEventListener("click", async () => {
    await runCommand(readValue());
    clearValue();
  });

  input.addEventListener("keydown", async (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      await runCommand(readValue());
      clearValue();
    }
  });

  clear?.addEventListener("click", () => {
    output.textContent = "";
    linesBuffered = 0;
  });

  stop?.addEventListener("click", () => {
    streamToken += 1;
    if (streaming) {
      appendLine("... stream stopped ...");
    }
    endStream();
  });

  dumpLog?.addEventListener("click", dumpQueenLog);
};
