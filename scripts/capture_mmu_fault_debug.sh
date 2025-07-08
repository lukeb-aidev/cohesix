#!/bin/bash
set -euo pipefail

OUTDIR="/home/ubuntu/cohesix/out/diag_mmu_fault_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUTDIR"

echo "📂 Saving diagnostics to $OUTDIR"

echo "👉 Dumping program headers..."
readelf -l /home/ubuntu/cohesix/out/bin/cohesix_root.elf > "$OUTDIR/cohesix_root_program_headers.txt"

echo "👉 Dumping section headers..."
readelf -S /home/ubuntu/cohesix/out/bin/cohesix_root.elf > "$OUTDIR/cohesix_root_sections.txt"

echo "👉 Dumping symbol table..."
readelf -s /home/ubuntu/cohesix/out/bin/cohesix_root.elf > "$OUTDIR/cohesix_root_symbols.txt"

echo "👉 Dumping full nm symbols..."
nm -n /home/ubuntu/cohesix/out/bin/cohesix_root.elf > "$OUTDIR/cohesix_root_nm.txt"

echo "👉 Dumping disassembly around fault region..."
objdump -d -M reg-names-raw /home/ubuntu/cohesix/out/bin/cohesix_root.elf > "$OUTDIR/cohesix_root_disasm.txt"

echo "👉 Copying QEMU debug logs..."
cp /home/ubuntu/cohesix/logs/qemu_debug_*.log "$OUTDIR/" 2>/dev/null || echo "⚠️ No QEMU logs found"

echo "✅ Diagnostics collected in $OUTDIR"
