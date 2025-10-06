/* Stub linker script to satisfy build.rs fallback when official sel4.ld is unavailable. */
OUTPUT_ARCH(aarch64)
ENTRY(_start)
SECTIONS {
  . = 0;
}
