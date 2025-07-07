# -----------------------
# Makefile for Cohesix
# -----------------------

.DEFAULT_GOAL := help

# Colors
GREEN := \033[0;32m
RESET := \033[0m

help:
	@echo ""
	@echo "Cohesix build targets:"
	@echo "  make dev         - Build with std for Linux dev"
	@echo "  make sel4        - Build for seL4 (no_std)"
	@echo "  make uefi        - Build minimal UEFI target (no_std)"
	@echo "  make clean       - Clean target directory"
	@echo ""

dev:
	@echo "$(GREEN)==> Building Cohesix with default features (std for Linux dev)...$(RESET)"
	cargo build --bin cohesix_root

sel4:
	@echo "$(GREEN)==> Building Cohesix for seL4 (no_std, using sel4-aarch64.json)...$(RESET)"
	cargo build --no-default-features --features "sel4" \
		--target cohesix_root/sel4-aarch64.json --bin cohesix_root

uefi:
	@echo "$(GREEN)==> Building Cohesix for minimal UEFI (no_std, aarch64-unknown-uefi)...$(RESET)"
	cargo build --no-default-features --features "kernel_bin minimal_uefi" \
		--target aarch64-unknown-uefi --bin kernel

clean:
	@echo "$(GREEN)==> Cleaning workspace...$(RESET)"
	cargo clean

full:
	@echo "$(GREEN)==> Building all Cohesix Rust binaries for seL4 (full pipeline)...$(RESET)"
	RUSTFLAGS="-C debuginfo=2" \
	cargo build --release --no-default-features --features "std,busybox,sel4,kernel_bin,minimal_uefi" \
		--target aarch64-unknown-linux-musl --bin cohesix_root

	@for bin in cohcc cohesix_build cohesix_cap cohesix_trace cohrun_cli cohagent cohrole cohrun cohup cohesix_root kernel logdemo init sel4_entry; do \
		BIN_PATH="target/aarch64-unknown-linux-musl/release/$$bin"; \
		if [ -f "$$BIN_PATH" ]; then \
			cp "$$BIN_PATH" out/bin/$$bin; \
			echo "✅ Copied $$bin to out/bin"; \
		else \
			echo "⚠️ $$bin not found at $$BIN_PATH"; \
		fi \
	done

stage:
	@echo "$(GREEN)==> Staging Python CLI scripts...$(RESET)"
	@for script in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do \
		if [ -f "bin/$$script" ]; then \
			cp "bin/$$script" out/bin/$$script; \
			cp "bin/$$script" out/usr/bin/$$script; \
			sed -i '1c #!/usr/bin/env python3' out/bin/$$script; \
			sed -i '1c #!/usr/bin/env python3' out/usr/bin/$$script; \
			chmod +x out/bin/$$script out/usr/bin/$$script; \
			echo "✅ Staged $$script"; \
		else \
			echo "⚠️ $$script not found"; \
		fi \
	done