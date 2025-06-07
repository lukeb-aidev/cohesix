# CLASSIFICATION: COMMUNITY
        # Filename: Makefile v0.5
# Date Modified: 2025-07-05
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
        @GOWORK=$(CURDIR)/go/go.work go test ./go/...

c/sel4/shim/boot_trampoline.o: c/sel4/shim/boot_trampoline.c
       $(CC:-clang?=cc) -I c/sel4/include -c $< -o $@

c/sel4/bootloader.o: c/sel4/bootloader.c
       $(CC:-clang?=cc) -I c/sel4/include -c $< -o $@

c-shims: c/sel4/shim/boot_trampoline.o c/sel4/bootloader.o
       @echo "🔧 Building C shims …"

help:
	@echo "Cohesix top‑level build targets:"
	@echo "  all       – run go-build, go-test and c-shims"
	@echo "  go-build  – vet Go workspace"
	@echo "  go-test   – run Go unit tests"
	@echo "  c-shims   – compile seL4 boot trampoline"