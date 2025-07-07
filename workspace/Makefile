# -----------------------
# Makefile for Cohesix
# -----------------------

.DEFAULT_GOAL := help

# Colors
GREEN := \033[0;32m
RESET := \033[0m

.PHONY: help sel4_root cli_tools cohcc cohesix_build cohesix_cap cohesix_trace cohagent cohrole cohrun cohup cohesix kernel_musl logdemo init full stage clean

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
	@set -e; \
	echo "$(GREEN)==> Building sel4_root...$(RESET)"; \
	mkdir -p out/bin; \
	RUSTFLAGS="-C link-arg=-Tlink.ld" cargo +nightly build -Z build-std=core,alloc --release \
		--manifest-path cohesix_root/Cargo.toml \
		--target cohesix_root/sel4-aarch64.json; \
	BIN_PATH="cohesix_root/target/sel4-aarch64/release/cohesix_root"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/sel4_root || echo "⚠️ sel4_root not found"


cohcc:
	@set -e; \
	echo "$(GREEN)==> Building cohcc...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohcc; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohcc"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohcc || echo "⚠️ cohcc not found"

cohesix_build:
	@set -e; \
	echo "$(GREEN)==> Building cohesix_build...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohesix_build; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohesix_build"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohesix_build || echo "⚠️ cohesix_build not found"

cohesix_cap:
	@set -e; \
	echo "$(GREEN)==> Building cohesix_cap...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohesix_cap; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohesix_cap"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohesix_cap || echo "⚠️ cohesix_cap not found"

cohesix_trace:
	@set -e; \
	echo "$(GREEN)==> Building cohesix_trace...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohesix_trace; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohesix_trace"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohesix_trace || echo "⚠️ cohesix_trace not found"


cohagent:
	@set -e; \
	echo "$(GREEN)==> Building cohagent...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohagent; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohagent"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohagent || echo "⚠️ cohagent not found"

cohrole:
	@set -e; \
	echo "$(GREEN)==> Building cohrole...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohrole; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohrole"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohrole || echo "⚠️ cohrole not found"

cohrun:
	@set -e; \
	echo "$(GREEN)==> Building cohrun...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohrun; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohrun"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohrun || echo "⚠️ cohrun not found"

cohup:
	@set -e; \
	echo "$(GREEN)==> Building cohup...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohup; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohup"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohup || echo "⚠️ cohup not found"

cohesix:
	@set -e; \
	echo "$(GREEN)==> Building cohesix...$(RESET)"; \
	mkdir -p out/bin; \
	cargo build --release --target aarch64-unknown-linux-musl --bin cohesix; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/cohesix"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/cohesix || echo "⚠️ cohesix not found"

kernel_musl:
	@set -e; \
	echo "$(GREEN)==> Building kernel (musl)...$(RESET)"; \
	mkdir -p out/bin; \
        cargo build --release --target aarch64-unknown-linux-musl --bin kernel --features "kernel_bin minimal_uefi"; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/kernel"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/kernel || echo "⚠️ kernel (musl) not found"

logdemo:
	@set -e; \
	echo "$(GREEN)==> Building logdemo...$(RESET)"; \
	mkdir -p out/bin; \
        cargo build --release --target aarch64-unknown-linux-musl --bin logdemo --features "minimal_uefi"; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/logdemo"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/logdemo || echo "⚠️ logdemo not found"

init:
	@set -e; \
	echo "$(GREEN)==> Building init...$(RESET)"; \
	mkdir -p out/bin; \
        cargo build --release --target aarch64-unknown-linux-musl --bin init --features "minimal_uefi"; \
	BIN_PATH="target/aarch64-unknown-linux-musl/release/init"; \
	[ -f "$$BIN_PATH" ] && cp "$$BIN_PATH" out/bin/init || echo "⚠️ init not found"

cli_tools: cohcc cohesix_build cohesix_cap cohesix_trace cohagent cohrole cohrun cohup cohesix kernel_musl logdemo init

full: sel4_root cli_tools

stage:
	@set -e; \
	echo "$(GREEN)==> Staging Python CLI scripts...$(RESET)"; \
	mkdir -p out/bin out/usr/bin; \
	for script in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do \
		if [ -f "bin/$$script" ]; then \
			cp "bin/$$script" out/bin/$$script; \
			cp "bin/$$script" out/usr/bin/$$script; \
			sed -i '1c #!/usr/bin/env python3' out/bin/$$script; \
			sed -i '1c #!/usr/bin/env python3' out/usr/bin/$$script; \
			chmod +x out/bin/$$script out/usr/bin/$$script; \
			echo "✅ Staged $$script"; \
		else \
			echo "⚠️ $$script not found"; \
		fi; \
	done

clean:
	@set -e; \
	echo "$(GREEN)==> Cleaning workspace...$(RESET)"; \
	cargo clean