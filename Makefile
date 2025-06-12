# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.8
# Date Modified: 2025-07-22
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

.PHONY: all go-build go-test c-shims help boot boot-x86_64 boot-aarch64 test

PLATFORM ?= $(shell uname -m)
.PHONY: all go-build go-test c-shims help cohrun cohbuild cohtrace cohcap

all: go-build go-test c-shims

go-build:
	@echo "🔧 Go vet …"
	@cd go && go vet ./...

go-test:
        @echo "🔧 Go unit tests …"
        @GOWORK=$(CURDIR)/go/go.work go test ./go/...

test:
@echo "🦀 cargo tests …"
@RUST_BACKTRACE=1 cargo test --release
@echo "🐍 pytest …"
@pytest -v
@echo "🐹 go tests …"
@GOWORK=$(CURDIR)/go/go.work go test ./go/...
@if [ -d build ]; then ctest --output-on-failure; fi

c/sel4/shim/boot_trampoline.o: c/sel4/shim/boot_trampoline.c
	$(CC:-clang?=cc) -I c/sel4/include -c $< -o $@

c/sel4/bootloader.o: c/sel4/bootloader.c
	$(CC:-clang?=cc) -I c/sel4/include -c $< -o $@

c-shims: c/sel4/shim/boot_trampoline.o c/sel4/bootloader.o
        @echo "🔧 Building C shims …"

boot-x86_64:
@echo "🏁 Building boot image for x86_64"
cargo build --release --target x86_64-unknown-linux-gnu

boot-aarch64:
@echo "🏁 Building boot image for aarch64"
cargo build --release --target aarch64-unknown-linux-gnu

boot:
$(MAKE) boot-$(PLATFORM)

help:
	@echo "Cohesix top‑level build targets:"
	@echo "  all       – run go-build, go-test and c-shims"
	@echo "  go-build  – vet Go workspace"
	@echo "  go-test   – run Go unit tests"
	@echo "  c-shims   – compile seL4 boot trampoline"
man: third_party/mandoc/mandoc
        cp third_party/mandoc/mandoc bin/cohman

cohrun:
       cargo run -p cohcli_tools --bin cohrun -- $(ARGS)

cohbuild:
       cargo run -p cohcli_tools --bin cohbuild -- $(ARGS)

cohtrace:
       cargo run -p cohcli_tools --bin cohtrace -- $(ARGS)

cohcap:
       cargo run -p cohcli_tools --bin cohcap -- $(ARGS)



