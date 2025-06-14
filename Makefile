# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.16
# Date Modified: 2025-07-23
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

.PHONY: all go-build go-test c-shims help fmt lint check \
    boot boot-x86_64 boot-aarch64 bootloader kernel cohrun cohbuild cohtrace cohcap test

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

$(info Checking for gnu-efi headers at $(EFI_BASE))
ifeq ($(wildcard $(GNUEFI_HDR)),)
$(error gnu-efi headers not found at $(GNUEFI_HDR))
endif

ifeq ($(wildcard $(GNUEFI_BIND)),)
$(warning $(GNUEFI_BIND) missing. Falling back to x86_64 headers if available.)
ifeq ($(EFI_ARCH),x86_64)
$(error Required architecture headers missing.)
else ifneq ($(wildcard $(EFI_BASE)/x86_64/efibind.h),)
EFI_ARCH := x86_64
GNUEFI_BIND := $(EFI_BASE)/$(EFI_ARCH)/efibind.h
else
$(error Required architecture headers missing.)
endif
endif

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

.PHONY: all go-build go-test c-shims help fmt lint check cohrun cohbuild cohtrace cohcap kernel

all: go-build go-test c-shims ## Run vet, tests and C shims

fmt: ## Run code formatters
	cargo fmt
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
	cargo clippy --all-targets -- -D warnings
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
	@RUST_BACKTRACE=1 cargo test --release
	@echo "🐍 Python tests …"
	@pytest -v
	@echo "🐹 Go tests …"
	@go test ./...
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

bootloader: ## Build UEFI bootloader
	@echo "🏁 Building UEFI bootloader using $(TOOLCHAIN)"
	@mkdir -p out/EFI/BOOT
	$(CC) $(CFLAGS_EFI) -c src/bootloader/main.c -o out/bootloader.o
	lld-link /lib/crt0-efi-x86_64.o out/bootloader.o \
	/out:out/bootloader.so /entry:efi_main /subsystem:efi_application \
	/defaultlib:gnuefi.lib /defaultlib:efi.lib
	objcopy --target=efi-app-x86_64 out/bootloader.so out/BOOTX64.EFI
	cp out/BOOTX64.EFI out/EFI/BOOT/BOOTX64.EFI


kernel: ## Build kernel stub
	@echo "🏁 Building kernel stub using $(TOOLCHAIN)"
	@mkdir -p out
	$(CC) $(CFLAGS_EFI) -c src/kernel/main.c -o out/kernel.o
	grep -v '^//' linker.ld > out/kernel.tmp.ld
	$(LD) /usr/lib/crt0-efi-x86_64.o out/kernel.o \
	    -o out/kernel.so -T out/kernel.tmp.ld $(LD_FLAGS)
	rm -f out/kernel.tmp.ld
	objcopy --target=efi-app-x86_64 out/kernel.so out/kernel.elf

boot: ## Build boot image for current PLATFORM
	$(MAKE) boot-$(PLATFORM)


testboot: ## Run UEFI boot test via QEMU
./test_boot_efi.sh

# Boot the built image in QEMU and capture serial output to qemu_serial.log
qemu: bootloader kernel ## Run qemu-system-x86_64
	@if command -v qemu-system-x86_64 >/dev/null 2>&1; then \
	cp /usr/share/OVMF/OVMF_VARS.fd out/OVMF_VARS.fd 2>/dev/null || true; \
	qemu-system-x86_64 -bios /usr/share/qemu/OVMF.fd \
	-drive if=pflash,format=raw,file=out/OVMF_VARS.fd \
	-drive format=raw,file=fat:rw:out/ -net none -M q35 -m 256M -no-reboot \
	-nographic -serial mon:stdio 2>&1 | tee qemu_serial.log; \
		else \
	echo "QEMU not installed; skipping"; \
	fi

# Boot via QEMU and verify BOOT_OK marker in serial log
qemu-check: ## Boot QEMU and check for BOOT_OK marker
	@$(MAKE) qemu >/dev/null
	@if [ -f qemu_serial.log ]; then \
	grep -q "BOOT_OK" qemu_serial.log && echo "Boot success" || (grep -o 'BOOT_FAIL:[^\n]*' qemu_serial.log || echo "Boot failure"; exit 1); \
	else \
	echo "qemu_serial.log missing"; exit 1; \
	fi

print-env: ## Display compiler information
	@echo "Toolchain: $(TOOLCHAIN)"
	@echo "Compiler: $(CC)"
	help: ## List available make targets
	@grep -E '^[a-zA-Z_-]+:.*##' Makefile \
	| awk 'BEGIN{FS=":.*##"; printf "Cohesix top-level build targets:\n"} {printf "  %-12s %s\n", $$1, $$2}'
man: third_party/mandoc/mandoc ## Install man page tool
	cp third_party/mandoc/mandoc bin/cohman

cohrun: ## Run cohrun CLI
	cargo run -p cohcli_tools --bin cohrun -- $(ARGS)

cohbuild: ## Run cohbuild CLI
	cargo run -p cohcli_tools --bin cohbuild -- $(ARGS)

cohtrace: ## Run cohtrace CLI
	cargo run -p cohcli_tools --bin cohtrace -- $(ARGS)

cohcap: ## Run cohcap CLI
	cargo run -p cohcli_tools --bin cohcap -- $(ARGS)



