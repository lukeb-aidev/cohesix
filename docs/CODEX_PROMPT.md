<!-- Author: Lukas Bower -->
# Web Codex Prompt (Root-task Bring-up)

Append this delta to the standing Web Codex prompt used for Cohesix. It
captures the current kernel build settings and the explicit asks for
feature verification now that `--no-default-features` is enforced.

## Build configuration (important)

We are now building `root-task` with:

```
cargo build \
  --target aarch64-unknown-none \
  --release \
  -p root-task \
  --no-default-features \
  --features kernel,bootstrap-trace,serial-console,net,net-console
```

## Tasks for the assistant

1. Inspect `apps/root-task/Cargo.toml` and the workspace `Cargo.toml` to
   enumerate:
   - The features defined for `root-task`.
   - Which features were previously enabled by default (`default = [...]`).
   - Which of those defaults are now disabled because of
     `--no-default-features`.
2. Ensure the following are explicitly enabled via feature wiring, not
   just inherited defaults:
   - The platform/board config for `virt` (MMIO, PL011, memory layout).
   - Root console / shell implementations that log:
     - `[console] PL011 console online`
     - `uart logger online`
     - `[console] starting root shell`
     - `Cohesix console ready`
   - Panic/fault logging paths expected on serial.
3. If features like `console`, `shell`, `platform-virt`, `panic-log`, or
   similar exist, either:
   - add them explicitly to the build features list above, or
   - include them in `root-task` defaults and avoid `--no-default-features`
     for this dev flow.
4. Validate that with `--no-default-features` and
   `--features kernel,bootstrap-trace,serial-console,net,net-console,...`
   (plus any extras discovered), QEMU:
   - boots,
   - prints expected boot phases,
   - brings up the PL011 root console, and
   - optionally surfaces the net-console/TCP console.
   Document the canonical feature set for a "full dev boot on virt with
   serial + net-console" using the command above or an updated feature
   list if necessary.
5. Add a comment in `Cargo.toml` or build docs that clearly states the
   supported feature combinations for:
   - serial-only dev boot
   - serial + net-console dev boot
   - and that using `--no-default-features` requires explicit feature
     lists.
