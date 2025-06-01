# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.3
# Date Modified: 2025-06-01
# Author: Lukas Bower
#
# ─────────────────────────────────────────────────────────────
# Cohesix · Top‑level Build Targets
#
#  • `make all`      – Go vet → Go tests → C shims
#  • `make go-build` – vet Go workspace
#  • `make go-test`  – run Go unit tests
#  • `make c-shims`  – compile seL4 boot trampoline object
#  • `make help`     – list targets
# ─────────────────────────────────────────────────────────────

.PHONY: all go-build go-test c-shims help

all: go-build go-test c-shims

go-build:
	@echo "🔧 Go vet …"
	@cd go && go vet ./...

go-test:
	@echo "🔧 Go unit tests …"
	@cd go && go test ./...

c-shims:
	@echo "🔧 Building C shims …"
	@$(CC:-clang?=cc) -I c/sel4/include \
		-c $(wildcard c/sel4/shim/*.c) -o c/sel4/shim/boot_trampoline.o

help:
	@echo "Cohesix top‑level build targets:"
	@echo "  all       – run go-build, go-test and c-shims"
	@echo "  go-build  – vet Go workspace"
	@echo "  go-test   – run Go unit tests"
	@echo "  c-shims   – compile seL4 boot trampoline"