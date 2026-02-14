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

  send.addEventListener("click", async () => {
    await runCommand(input.value);
    input.value = "";
  });

  input.addEventListener("keydown", async (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      await runCommand(input.value);
      input.value = "";
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
};
