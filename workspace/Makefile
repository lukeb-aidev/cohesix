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
	cargo build

sel4:
	@echo "$(GREEN)==> Building Cohesix for seL4 (no_std)...$(RESET)"
	cargo build --no-default-features --features "sel4"

uefi:
	@echo "$(GREEN)==> Building Cohesix for minimal UEFI (no_std)...$(RESET)"
	cargo build --no-default-features --features "kernel_bin minimal_uefi"

clean:
	@echo "$(GREEN)==> Cleaning workspace...$(RESET)"
	cargo clean