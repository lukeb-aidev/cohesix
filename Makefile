# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.2
# Date Modified: 2025-06-01
# Author: Lukas Bower
#
# ─────────────────────────────────────────────────────────────
# Cohesix · Top‑level Build Targets
#
#  • `make all`      – run Go vet + compile C shims
#  • `make go-build` – vet Go workspace
#  • `make c-shims`  – compile seL4 boot trampoline object
#  • `make help`     – list targets
# ─────────────────────────────────────────────────────────────

.PHONY: all go-build c-shims help

all: go-build c-shims

go-build:
	@echo "🔧 Go vet …"
	@cd go && go vet ./...

c-shims:
	@echo "🔧 Building C shims …"
	@$(CC:-clang?=cc) -c c/sel4/shim/boot_trampoline.c -o c/sel4/shim/boot_trampoline.o

help:
	@echo "Cohesix top‑level build targets:"
	@echo "  all       – run go-build and c-shims"
	@echo "  go-build  – vet Go workspace"
	@echo "  c-shims   – compile seL4 boot trampoline"
