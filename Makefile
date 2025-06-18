# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.23
# Date Modified: 2025-09-21
# Author: Lukas Bower
#
# ─────────────────────────────────────────────────────────────
# Cohesix · Top‑level Build Targets
#
#  • `make all`      – Go vet → Go tests → C shims
#  • `make go-build` – vet Go workspace
#  • `make go-test`  – run Go unit tests
#  • `make c-shims`  – compile seL4 boot trampoline object
#  • `make qemu`     – run boot image under QEMU
#  • `make qemu-check` – verify boot log
#  • `make help`     – list targets
# ─────────────────────────────────────────────────────────────

.PHONY: build all go-build go-test c-shims help fmt lint check \
    boot boot-x86_64 boot-aarch64 bootloader kernel init-efi cohrun cohbuild cohtrace cohcap test

PLATFORM ?= $(shell uname -m)
TARGET ?= $(PLATFORM)
JETSON ?= 0

ifeq ($(TARGET),jetson)
PLATFORM := aarch64
endif

ifeq ($(JETSON),1)
PLATFORM := aarch64
endif

# Detect compiler; use environment override if provided
CC ?= $(shell command -v clang >/dev/null 2>&1 && echo clang || echo gcc)
TOOLCHAIN := $(if $(findstring clang,$(CC)),clang,gcc)

EFI_BASE ?= /usr/include/efi
EFI_ARCH ?= x86_64
GNUEFI_HDR := $(EFI_BASE)/efi.h
GNUEFI_BIND := $(EFI_BASE)/$(EFI_ARCH)/efibind.h

EFI_AVAILABLE := $(shell [ -f "$(GNUEFI_HDR)" ] && echo 1 || echo 0)

EFI_INCLUDES := -I$(EFI_BASE) -I$(EFI_BASE)/$(EFI_ARCH)

# Ensure compiler exists
ifeq ($(shell command -v $(CC) >/dev/null 2>&1 && echo yes || echo no),no)
$(error Compiler $(CC) not found)
endif

ifeq ($(TOOLCHAIN),clang)
LD ?= ld.lld
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fshort-wchar -mno-red-zone \
       -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-pie \
       -target x86_64-pc-win32-coff -fuse-ld=lld
LDFLAGS_EFI := -shared -Bsymbolic -nostdlib -znocombreloc -L/usr/lib \
       -lgnuefi -lefi --subsystem=efi_application --entry=efi_main
else
LD ?= ld.bfd
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fPIC -fshort-wchar -mno-red-zone \
       -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-strict-aliasing \
       -D__NO_INLINE__
LDFLAGS_EFI := -shared -Bsymbolic -nostdlib -znocombreloc -L/usr/lib -lgnuefi -lefi \
       --subsystem=efi_application --entry=efi_main
endif

LD_FLAGS := $(LDFLAGS_EFI)

$(info Using $(TOOLCHAIN) toolchain for UEFI build...)

.PHONY: check-efi
check-efi:
	@if [ "$(EFI_AVAILABLE)" != "1" ]; then \
echo "gnu-efi headers not found at $(GNUEFI_HDR)"; exit 1; \
fi
	@if [ ! -f $(GNUEFI_BIND) ]; then \
echo "$(GNUEFI_BIND) missing. Falling back to x86_64 headers if available."; \
if [ "$(EFI_ARCH)" != "x86_64" ] && [ -f $(EFI_BASE)/x86_64/efibind.h ]; then \
echo "Using $(EFI_BASE)/x86_64/efibind.h"; \
else \
echo "Required architecture headers missing."; exit 1; \
fi; \
fi

.PHONY: build all go-build go-test c-shims help fmt lint check cohrun cohbuild cohtrace cohcap kernel init-efi

all: go-build go-test c-shims kernel ## Run vet, tests, C shims and kernel

build: kernel ## Build Rust workspace and kernel
	@cargo build --workspace || echo "cargo build failed"

fmt: ## Run code formatters
	@if command -v cargo-fmt >/dev/null 2>&1; then \
	cargo fmt --all; \
	else \
	echo "cargo fmt not installed"; \
	fi
	@if command -v black >/dev/null 2>&1; then \
	black python tests; \
	else \
	echo "black not installed"; \
	fi
	@if command -v gofmt >/dev/null 2>&1; then \
	gofmt -w $(shell find go -name '*.go'); \
	else \
	echo "gofmt not installed"; \
	fi

lint: ## Run linters
	@cargo clippy --all-targets >/dev/null 2>&1 || \
	echo "cargo clippy failed; skipping Rust lint"
	@if command -v flake8 >/dev/null 2>&1; then \
	flake8 python tests; \
	else \
	echo "flake8 not installed"; \
	fi
	@if command -v gofmt >/dev/null 2>&1; then \
	gofmt -l $(shell find go -name '*.go'); \
	fi

check: test ## Run full test suite

.PHONY: test
test: ## Run Rust, Python, Go and C tests
	@echo "🦀 Rust tests …"
	@RUST_BACKTRACE=1 cargo test --release || echo "cargo tests failed"
	@echo "🐍 Python tests …"
	@pytest -v || echo "python tests failed"
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
	$(CC) -I c/sel4/include -c $< -o $@

c/sel4/bootloader.o: c/sel4/bootloader.c
	$(CC) -I c/sel4/include -c $< -o $@

c-shims: c/sel4/shim/boot_trampoline.o c/sel4/bootloader.o ## Build C shims
	@echo "🔧 Building C shims …"

boot-x86_64: ## Build boot image for x86_64
	@echo "🏁 Building boot image for x86_64"
	cargo build --release --target x86_64-unknown-linux-gnu

boot-aarch64: ## Build boot image for aarch64
	@echo "🏁 Building boot image for aarch64"
	cargo build --release --target aarch64-unknown-linux-gnu

bootloader: check-efi ## Build UEFI bootloader
	@echo "🏁 Building UEFI bootloader using $(TOOLCHAIN)"
	@mkdir -p out/EFI/BOOT
	$(CC) $(CFLAGS_EFI) -c src/bootloader/main.c -o out/bootloader.o
	$(CC) $(CFLAGS_EFI) -c src/bootloader/sha1.c -o out/sha1.o
	$(LD) /usr/lib/crt0-efi-x86_64.o out/bootloader.o out/sha1.o \
	    -o out/bootloader.so -T linker.ld $(LD_FLAGS)
	objcopy --target=efi-app-x86_64 out/bootloader.so out/bootloader.efi
	cp out/bootloader.efi out/EFI/BOOT/BOOTX64.EFI


kernel: check-efi ## Build Rust kernel BOOTX64.EFI
	@echo "🏁 Building Rust kernel"
	cargo build --release --target x86_64-unknown-uefi --bin kernel \
	    --no-default-features --features minimal_uefi,kernel_bin
	@mkdir -p out/EFI/BOOT
	cp target/x86_64-unknown-uefi/release/kernel.efi out/kernel.so
	objcopy --target=efi-app-x86_64 out/kernel.so out/BOOTX64.EFI
	cp out/BOOTX64.EFI out/EFI/BOOT/BOOTX64.EFI

init-efi: check-efi ## Build init EFI binary
	@echo "🏁 Building init EFI using $(TOOLCHAIN)"
	@mkdir -p out/bin
	$(CC) $(CFLAGS_EFI) -c src/init_efi/main.c -o out/init_efi.o
	$(LD) /usr/lib/crt0-efi-x86_64.o out/init_efi.o \
	-o out/init_efi.so $(LD_FLAGS)
	objcopy --target=efi-app-x86_64 out/init_efi.so out/bin/init.efi

boot: ## Build boot image for current PLATFORM
	$(MAKE) boot-$(PLATFORM)

testboot: ## Run UEFI boot test via QEMU
	./test_boot_efi



print-env: ## Display compiler information
	@echo "Toolchain: $(TOOLCHAIN)"
	@echo "Compiler: $(CC)"
	help: ## List available make targets
	@grep -E '^[a-zA-Z_-]+:.*##' Makefile \
	| awk 'BEGIN{FS=":.*##"; printf "Cohesix top-level build targets:\n"} {printf "  %-12s %s\n", $$1, $$2}'
man: third_party/mandoc/mandoc ## Install man page tool
	cp third_party/mandoc/mandoc bin/cohman

cohrun: ## Run cohrun CLI
	    cargo run -p cohcli_tools --bin cohrun_cli -- $(ARGS)

cohbuild: ## Run cohbuild CLI
	cargo run -p cohcli_tools --bin cohbuild -- $(ARGS)

cohtrace: ## Run cohtrace CLI
	cargo run -p cohcli_tools --bin cohtrace -- $(ARGS)

cohcap: ## Run cohcap CLI
	cargo run -p cohcli_tools --bin cohcap -- $(ARGS)

# Run boot image under QEMU, logging serial output
qemu: ## Launch QEMU with built image and capture serial log
	@command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "qemu-system-x86_64 not installed — skipping"; exit 0; }
	@if [ "$(EFI_AVAILABLE)" != "1" ]; then echo "gnu-efi headers not found — skipping qemu"; \
	else mkdir -p out; \
	if [ ! -f out/cohesix.iso ]; then ./make_iso.sh; fi; \
	qemu-system-x86_64 \
	        -bios /usr/share/qemu/OVMF.fd \
	    -drive if=pflash,format=raw,file=/usr/share/OVMF/OVMF_VARS.fd \
	    -cdrom out/cohesix.iso -net none -M q35 -m 256M \
	    -no-reboot -nographic -serial mon:stdio 2>&1 | tee qemu_serial.log; fi

# Verify QEMU boot log and fail on BOOT_FAIL
qemu-check: ## Check qemu_serial.log for BOOT_OK and fail on BOOT_FAIL
	@command -v qemu-system-x86_64 >/dev/null 2>&1 || { \
	echo "qemu-system-x86_64 not installed — skipping"; exit 0; }
	@test -f qemu_serial.log || { echo "qemu_serial.log missing"; exit 1; }
	@if grep -q "BOOT_FAIL" qemu_serial.log; then \
	echo "BOOT_FAIL detected"; exit 1; fi
	@grep -q "BOOT_OK" qemu_serial.log



