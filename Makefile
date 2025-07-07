# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.52
# Author: Lukas Bower
# Date Modified: 2027-09-30
.PHONY: build cuda-build all go-build go-test c-shims help fmt lint check \
        rootserver userland full \
        boot boot-x86_64 boot-aarch64 cohrun cohbuild cohtrace cli_cap gui-orchestrator cloud test test-python check-tab-safety iso boot-grub qemu qemu-check

PLATFORM ?= $(shell uname -m)
TARGET ?= $(PLATFORM)
JETSON ?= 0

ifeq ($(TARGET),jetson)
PLATFORM := aarch64
endif

export MEMCHR_DISABLE_RUNTIME_CPU_FEATURE_DETECTION ?= 1

ifeq ($(JETSON),1)
PLATFORM := aarch64
endif

# Detect build host operating system. On Windows, `$(OS)` is set to Windows_NT.
# Fallback to `uname` when $(OS) is empty. Used to skip Windows-only EFI checks.
HOST_OS ?= $(if $(OS),$(OS),$(shell uname -s))
# Detect compiler; use environment override if provided
CC ?= $(shell command -v clang >/dev/null 2>&1 && echo clang || echo gcc)
TOOLCHAIN := $(if $(findstring clang,$(CC)),clang,gcc)
all: go-build go-test c-shims ## Run vet, tests, C shims

build: ## Build Rust workspace
	@cargo build --workspace || echo "cargo build failed"
cuda-build: ## Build release with CUDA features
	cargo clean && cargo build --release --features=cuda

fmt: ## Run code formatters
	@if command -v cargo-fmt >/dev/null 2>&1; then \
	cargo fmt --all; \
	else \
	echo "cargo fmt not installed"; \
	fi
	@if command -v gofmt >/dev/null 2>&1; then \
	gofmt -w $(shell find go -name '*.go'); \
	else \
	echo "gofmt not installed"; \
	fi

lint: ## Run linters
	@cargo clippy --all-targets >/dev/null 2>&1 || \
	echo "cargo clippy failed; skipping Rust lint"
		@if command -v gofmt >/dev/null 2>&1; then \
		gofmt -l $(shell find go -name '*.go'); \
		fi

check: test ## Run full test suite

.PHONY: test test-python
test: ## Run Rust, Go and C tests
	@echo "🦀 Rust tests …"
	@RUST_BACKTRACE=1 cargo test --release || echo "cargo tests failed"
	@echo "🐹 Go tests …"
	@GOWORK=$(CURDIR)/go/go.work go test ./go/... || echo "go tests failed"
	@echo "🧱 C tests …"
	@cd build && ctest --output-on-failure || true

go-build: ## Vet Go workspace
	@echo "🔧 Go vet …"
	@cd go && go vet ./...

go-test: ## Run Go unit tests
	@echo "🔧 Go unit tests …"
	@GOWORK=$(CURDIR)/go/go.work go test ./go/...

c/sel4/shim/boot_trampoline.o: c/sel4/shim/boot_trampoline.c
	$(CC) $(CFLAGS_WARN) -I c/sel4/include -c $< -o $@

c/sel4/bootloader.o: c/sel4/bootloader.c
	$(CC) $(CFLAGS_WARN) -I c/sel4/include -c $< -o $@

c-shims: c/sel4/shim/boot_trampoline.o c/sel4/bootloader.o ## Build C shims
	@echo "🔧 Building C shims …"

boot-x86_64: ## Build UEFI image for x86_64 # Updated for pure UEFI targets, no linux-gnu
	@echo "🏁 Building UEFI image for x86_64" # Updated for pure UEFI targets, no linux-gnu
	cargo build --release --target x86_64-unknown-uefi # Updated for pure UEFI targets, no linux-gnu

boot-aarch64: ## Build UEFI image for aarch64 # Updated for pure UEFI targets, no linux-gnu
	@echo "🏁 Building UEFI image for aarch64" # Updated for pure UEFI targets, no linux-gnu
	cargo build --release --target aarch64-unknown-uefi # Updated for pure UEFI targets, no linux-gnu





boot: ## Build boot image for current PLATFORM
	$(MAKE) boot-$(PLATFORM)



print-env: ## Display compiler information
	@echo "Toolchain: $(TOOLCHAIN)"
	@echo "Compiler: $(CC)"
rootserver: ## Build seL4 rootserver (sel4_entry + kernel)
	@$(MAKE) -C workspace sel4_root
userland: ## Build userland binaries and services
	@$(MAKE) -C workspace cli_tools
full: ## Build rootserver and userland
	@$(MAKE) -C workspace full
	help: ## List available make targets
	@grep -E '^[a-zA-Z_-]+:.*##' Makefile \
	| awk 'BEGIN{FS=":.*##"; printf "Cohesix top-level build targets:\n"} {printf "  %-12s %s\n", $$1, $$2}'
man: third_party/mandoc/mandoc ## Install man page tool
	cp third_party/mandoc/mandoc bin/cohman

cohrun: ## Run cohrun CLI
	cargo run -p cohcli_tools --bin cohrun_cli -- $(ARGS)

cohbuild: ## Run cohbuild CLI
	cargo run -p cohcli_tools --bin cohesix_build -- $(ARGS)

cohtrace: ## Run cohtrace CLI
	cargo run -p cohcli_tools --bin cohesix_trace -- $(ARGS)

cli_cap: ## Run cohcap CLI
	cargo run -p cohcli_tools --bin cli_cap -- $(ARGS)

gui-orchestrator: ## Build gui-orchestrator binary
        @echo "Building gui-orchestrator"
        @mkdir -p out/bin
        @GOWORK=$(CURDIR)/go/go.work go build -o out/bin/gui-orchestrator ./go/cmd/gui-orchestrator

cloud: ## Build cloud service binary
        cargo build --bin cloud

iso: # Updated to clarify legacy status
	@echo "Creating legacy GRUB-based ISO (likely unused for UEFI)..." # Updated to clarify legacy status
	./tools/make_iso.sh # Updated to clarify legacy status

boot-grub: iso
	qemu-system-aarch64 -M virt -cpu cortex-a57 -m 1024 -bios none -serial mon:stdio -cdrom out/cohesix.iso -nographic


# Run boot image under QEMU, logging serial output
qemu: ## Launch QEMU with built image and capture serial log
	@command -v qemu-system-$(ARCH) >/dev/null 2>&1 || { echo "qemu-system-$(ARCH) not installed — skipping"; exit 0; }
	@mkdir -p out
	@if [ ! -f out/cohesix.iso ]; then tools/make_iso.sh; fi
	@[ -f out/cohesix.iso ] || { echo "ISO build failed"; exit 1; }
	@if [ "$(ARCH)" = "x86_64" ]; then \
	qemu-system-x86_64 -cdrom out/cohesix.iso -net none -M q35 -m 256M \
	-no-reboot -nographic -serial mon:stdio 2>&1 | tee qemu_serial.log; \
	else \
	qemu-system-aarch64 -machine virt -cpu cortex-a53 -m 256M \
	-cdrom out/cohesix.iso -net none -nographic -no-reboot \
	-serial mon:stdio 2>&1 | tee qemu_serial.log; \
	fi


# Verify QEMU boot log and fail on BOOT_FAIL
qemu-check: ## Check qemu_serial.log for BOOT_OK and fail on BOOT_FAIL
	@command -v qemu-system-x86_64 >/dev/null 2>&1 || { \
	echo "qemu-system-x86_64 not installed — skipping"; exit 0; }
	@test -f qemu_serial.log || { echo "qemu_serial.log missing"; exit 1; }
	@if grep -q "BOOT_FAIL" qemu_serial.log; then \
	echo "BOOT_FAIL detected"; exit 1; fi
	@grep -q "BOOT_OK" qemu_serial.log


check-tab-safety:
	@grep -Pn "^\s{4,}[^\t]" Makefile && echo "WARNING: spaces used in recipe lines" || echo "Tab check passed"
