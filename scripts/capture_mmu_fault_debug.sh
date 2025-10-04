#!/bin/bash
# Author: Lukas Bower
# Backlog Epic: InvestigateCohesixRootBootFail-274 (MMU fault triage)
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
trace_root=${COHESIX_TRACE_TMP:-${TMPDIR:-"${repo_root}/out"}}
outdir="${trace_root%/}/diag_mmu_fault_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$outdir"

echo "📂 Saving diagnostics to $outdir"

elf_path="$repo_root/out/bin/cohesix_root.elf"
if [[ ! -f "$elf_path" ]]; then
  echo "❌ Expected ELF not found at $elf_path" >&2
  exit 1
fi

log_dir="$repo_root/logs"

echo "👉 Dumping program headers..."
readelf -l "$elf_path" > "$outdir/cohesix_root_program_headers.txt"

echo "👉 Dumping section headers..."
readelf -S "$elf_path" > "$outdir/cohesix_root_sections.txt"

echo "👉 Dumping symbol table..."
readelf -s "$elf_path" > "$outdir/cohesix_root_symbols.txt"

echo "👉 Dumping full nm symbols..."
nm -n "$elf_path" > "$outdir/cohesix_root_nm.txt"

echo "👉 Dumping disassembly around fault region..."
objdump -d -M reg-names-raw "$elf_path" > "$outdir/cohesix_root_disasm.txt"

if [[ -d "$log_dir" ]]; then
  echo "👉 Copying QEMU debug logs..."
  shopt -s nullglob
  qemu_logs=("$log_dir"/qemu_debug_*.log)
  if [[ ${#qemu_logs[@]} -gt 0 ]]; then
    cp "${qemu_logs[@]}" "$outdir/"
  else
    echo "⚠️ No QEMU logs found under $log_dir"
  fi
  shopt -u nullglob
else
  echo "⚠️ Log directory $log_dir not found"
fi

echo "✅ Diagnostics collected in $outdir"
