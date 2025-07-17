# CLASSIFICATION: COMMUNITY
# Filename: capture_and_push_debug.sh v0.3
# Author: Lukas Bower
# Date Modified: 2027-12-31
#!/usr/bin/env bash
set -euo pipefail

NOW=$(date +%Y%m%d_%H%M%S)
DIAG_DIR="out/diag_mmu_fault_${NOW}"
mkdir -p "$DIAG_DIR"
warnings_found=0

echo "📂 Locating cohesix_root ELF..."
COHESIX_ELF=$(find workspace -type f -name cohesix_root -printf "%T@ %p\n" | sort -n | tail -1 | cut -d' ' -f2 || true)
if [ -z "$COHESIX_ELF" ]; then
  COHESIX_ELF=$(find . -type f -name cohesix_root -printf "%T@ %p\n" | sort -n | tail -1 | cut -d' ' -f2 || true)
fi
if [ -z "$COHESIX_ELF" ]; then
  echo "❌ Could not find built cohesix_root ELF. Run cargo build first."
  exit 1
fi
echo "✅ Found ELF at $COHESIX_ELF"

echo "👉 Dumping program headers..."
readelf -l "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_program_headers.txt"

echo "👉 Dumping section headers..."
readelf -S "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_sections.txt"

echo "👉 Dumping symbol table..."
readelf -s "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_symbols.txt"

echo "👉 Dumping full nm symbols..."
nm -n "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_nm.txt"

echo "👉 Dumping demangled nm symbols..."
nm -n --demangle "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_nm_demangled.txt"

echo "👉 Checking for undefined symbols..."
nm -A -n "$COHESIX_ELF" | grep ' U ' > "$DIAG_DIR/cohesix_root_undefined_symbols.txt" || true
if [ -s "$DIAG_DIR/cohesix_root_undefined_symbols.txt" ]; then
  echo "⚠️ Undefined symbols detected:"
  cat "$DIAG_DIR/cohesix_root_undefined_symbols.txt"
  warnings_found=1
else
  echo "✅ No undefined symbols"
fi

echo "👉 Dumping verbose symbol table..."
readelf -Ws "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_symbols_verbose.txt"
grep -E ' printf| malloc| free| strcmp| strcpy| memcpy' "$DIAG_DIR/cohesix_root_symbols_verbose.txt" > "$DIAG_DIR/cohesix_root_libc_symbols.txt" || true
if [ -s "$DIAG_DIR/cohesix_root_libc_symbols.txt" ]; then
  echo "⚠️ Potential libc/musl symbols detected:" 
  cat "$DIAG_DIR/cohesix_root_libc_symbols.txt"
  warnings_found=1
else
  echo "✅ No libc/musl symbols found"
fi

echo "👉 Dumping full disassembly with llvm-objdump..."
if command -v llvm-objdump &> /dev/null; then
  llvm-objdump -d "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_full_disasm_llvm.txt"
  echo "✅ llvm-objdump completed."
else
  echo "⚠️ llvm-objdump not found. Skipping."
fi

grep -nE '\b(call|bl)\b' "$DIAG_DIR/cohesix_root_full_disasm.txt" | grep -vE '(seL4_|coh_|core::|alloc::|rust_begin_unwind)' > "$DIAG_DIR/cohesix_root_suspicious_calls.txt" || true
if [ -s "$DIAG_DIR/cohesix_root_suspicious_calls.txt" ]; then
  echo "⚠️ Suspicious external calls detected:"
  cat "$DIAG_DIR/cohesix_root_suspicious_calls.txt" | head -n 20
  warnings_found=1
else
  echo "✅ No suspicious external calls found"
fi

echo "👉 Dumping full readelf..."
readelf -a "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_full_readelf.txt"

echo "👉 Dumping objdump sections..."
objdump -h "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_objdump_sections.txt"

echo "👉 Copying sel4 target JSON and linker script if available..."
cp cohesix_root/sel4-aarch64.json "$DIAG_DIR/" 2>/dev/null || true
cp cohesix_root/link.ld "$DIAG_DIR/" 2>/dev/null || true

echo "👉 Copying latest QEMU log..."
LATEST_QEMU_LOG=$(ls -t /home/ubuntu/cohesix/logs/qemu_debug_*.log | head -n1 || true)
LATEST_QEMU_SERLOG=$(ls -t /home/ubuntu/cohesix/logs/qemu_serial_*.log | head -n1 || true)
if [ -f "$LATEST_QEMU_LOG" ]; then
  cp "$LATEST_QEMU_LOG" "$DIAG_DIR/"
  cp "$LATEST_QEMU_SERLOG" "$DIAG_DIR/"
else
  echo "⚠️ No QEMU logs found."
fi

echo "✅ Diagnostics saved."

echo "📂 Staging diagnostics and this script for git..."
git add -f "$DIAG_DIR" capture_and_push_debug.sh

echo "✅ Committing..."
git commit -m "Add MMU fault diagnostics at $NOW"

echo "🚀 Pushing to remote..."
git push

echo "✅ Done."

echo "📂 All diagnostics are in $DIAG_DIR"

if [ "$warnings_found" -eq 0 ]; then
  echo "✅ ELF checks passed."
else
  echo "❌ Warnings detected during ELF analysis."
fi

