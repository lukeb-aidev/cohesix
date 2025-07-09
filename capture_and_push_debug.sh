#!/usr/bin/env bash
set -euo pipefail

NOW=$(date +%Y%m%d_%H%M%S)
DIAG_DIR="out/diag_mmu_fault_${NOW}"
mkdir -p "$DIAG_DIR"

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


echo "👉 Dumping disassembly..."
objdump -d "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_disasm.txt"

echo "👉 Dumping full readelf..."
readelf -a "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_full_readelf.txt"

echo "👉 Dumping objdump sections..."
objdump -h "$COHESIX_ELF" > "$DIAG_DIR/cohesix_root_objdump_sections.txt"

echo "👉 Copying sel4 target JSON and linker script if available..."
cp cohesix_root/sel4-aarch64.json "$DIAG_DIR/" 2>/dev/null || true
cp cohesix_root/link.ld "$DIAG_DIR/" 2>/dev/null || true

echo "👉 Copying latest QEMU log..."
LATEST_QEMU_LOG=$(ls -t /home/ubuntu/cohesix/logs/qemu_debug_*.log | head -n1 || true)
if [ -f "$LATEST_QEMU_LOG" ]; then
  cp "$LATEST_QEMU_LOG" "$DIAG_DIR/"
else
  echo "⚠️ No QEMU log found."
fi

echo "✅ Diagnostics saved."

echo "📂 Staging diagnostics and this script for git..."
git add -f "$DIAG_DIR" capture_and_push_debug.sh

echo "✅ Committing..."
git commit -m "Add MMU fault diagnostics at $NOW"

echo "🚀 Pushing to remote..."
git push

echo "✅ Done."
