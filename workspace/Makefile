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
	@echo "  make sel4_root   - Build seL4 rootserver binaries (no_std, custom target)"
	@echo "  make cli_tools   - Build CLI tools and services with musl (std)"
	@echo "  make full        - Build everything (rootserver + CLI tools)"
	@echo "  make stage       - Stage Python CLI scripts"
	@echo "  make clean       - Clean target directory"
	@echo ""

sel4_root:
	@echo "$(GREEN)==> Building Cohesix seL4 rootserver and sel4_entry (no_std)...$(RESET)"
	RUSTFLAGS="-C debuginfo=2" \
	cargo build -Z build-std=core,alloc --release --no-default-features --features "sel4,kernel_bin,minimal_uefi" \
		--target cohesix_root/sel4-aarch64.json --bin sel4_entry --bin kernel
	@for bin in sel4_entry kernel; do \
		BIN_PATH="target/$(shell basename cohesix_root/sel4-aarch64.json)/release/$$bin"; \
		if [ -f "$$BIN_PATH" ]; then \
			cp "$$BIN_PATH" out/bin/$$bin; \
			echo "✅ Copied $$bin to out/bin"; \
		else \
			echo "⚠️ $$bin not found at $$BIN_PATH"; \
		fi \
	done

cli_tools:
	@echo "$(GREEN)==> Building Cohesix CLI tools and services (musl)...$(RESET)"
	RUSTFLAGS="-C debuginfo=2" \
	cargo build --release --target aarch64-unknown-linux-musl --bins
	@for bin in cohcc cohesix_build cohesix_cap cohesix_trace cohrun_cli cohagent cohrole cohrun cohup cohesix kernel logdemo init; do \
		BIN_PATH="target/aarch64-unknown-linux-musl/release/$$bin"; \
		if [ -f "$$BIN_PATH" ]; then \
			cp "$$BIN_PATH" out/bin/$$bin; \
			echo "✅ Copied $$bin to out/bin"; \
		else \
			echo "⚠️ $$bin not found at $$BIN_PATH"; \
		fi \
	done

full: sel4_root cli_tools

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

clean:
	@echo "$(GREEN)==> Cleaning workspace...$(RESET)"
	cargo clean