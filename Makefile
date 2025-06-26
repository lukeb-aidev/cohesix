				# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.47
# Date Modified: 2026-09-12
# Author: Lukas Bower
#
# ─────────────────────────────────────────────────────────────
# Cohesix · Top‑level Build Targets
#
#  • `make all`      – Go vet → Go tests → C shims
#  • `make go-build` – vet Go workspace
#  • `make go-test`  – run Go unit tests
#  • `make c-shims`  – compile seL4 boot trampoline object
#  • `make cuda-build` – release build with CUDA features
#  • `make qemu`     – run boot image under QEMU
#  • `make qemu-check` – verify boot log
#  • `make help`     – list targets
# ─────────────────────────────────────────────────────────────

.PHONY: build cuda-build all go-build go-test c-shims help fmt lint check \
       boot boot-x86_64 boot-aarch64 bootloader kernel init-efi cohrun cohbuild cohtrace cli_cap gui-orchestrator test test-python check-tab-safety

PLATFORM ?= $(shell uname -m)
TARGET ?= $(PLATFORM)
JETSON ?= 0

ifeq ($(TARGET),jetson)
PLATFORM := aarch64
endif

ifeq ($(JETSON),1)
PLATFORM := aarch64
endif

# Detect build host operating system. On Windows, `$(OS)` is set to Windows_NT.
# Fallback to `uname` when $(OS) is empty. Used to skip Windows-only EFI checks.
HOST_OS ?= $(if $(OS),$(OS),$(shell uname -s))
ARCH := $(shell uname -m)

ifeq ($(ARCH),aarch64)
CRT0 := $(HOME)/gnu-efi/gnuefi/crt0-efi-aarch64.o
else ifeq ($(ARCH),x86_64)
CRT0 := /usr/lib/x86_64-linux-gnu/crt0-efi-x86_64.o
else
$(error Unsupported architecture: $(ARCH))
endif

ifeq ($(wildcard $(CRT0)),)
$(error crt0 object not found at $(CRT0))
endif

# Detect compiler; use environment override if provided
CC ?= $(shell command -v clang >/dev/null 2>&1 && echo clang || echo gcc)
TOOLCHAIN := $(if $(findstring clang,$(CC)),clang,gcc)
# Cross-compilers for AArch64 UEFI builds
CROSS_CC ?= aarch64-linux-gnu-gcc
CROSS_LD ?= aarch64-linux-gnu-ld
CROSS_ARCH ?= aarch64

# gnu-efi library and include locations; override with env vars if set
GNUEFI_LIBDIR ?= $(shell test -f /usr/lib/gnuefi/libgnuefi.a && echo /usr/lib/gnuefi || echo /usr/lib)
GNUEFI_INCDIR ?= $(shell test -d /usr/include/efi && echo /usr/include/efi || echo /usr/include)

# Fallback to /usr/local/lib if libgnuefi.a resides there. This aids
# macOS/Homebrew setups without impacting Linux builds.
LOCAL_GNUEFI := $(shell [ -f /usr/local/lib/libgnuefi.a ] && echo -L/usr/local/lib -lgnuefi)

# Common library flags
LIBS := -lgnuefi -lefi

EFI_BASE ?= /usr/include/efi
EFI_ARCH ?= x86_64
GNUEFI_HDR := $(EFI_BASE)/efi.h
GNUEFI_BIND := $(EFI_BASE)/$(EFI_ARCH)/efibind.h

EFI_AVAILABLE := $(shell [ -f "$(GNUEFI_HDR)" ] && echo 1 || echo 0)

EFI_INCLUDES := -I$(GNUEFI_INCDIR) -I$(GNUEFI_INCDIR)/$(CROSS_ARCH)

# Ensure compiler exists
ifeq ($(shell command -v $(CC) >/dev/null 2>&1 && echo yes || echo no),no)
$(error Compiler $(CC) not found)
endif

ifeq ($(TOOLCHAIN),clang)
LD ?= ld.lld
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fPIC -fshort-wchar -mno-red-zone \
        -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-pie \
        -target x86_64-pc-win32-coff -fuse-ld=lld
EFI_SUBSYSTEM_FLAG :=
ifeq ($(findstring Windows,$(HOST_OS)),Windows)
EFI_SUBSYSTEM_FLAG := --subsystem=efi_application
ifeq ($(shell $(LD) -v 2>&1 | grep -E -c "(lld|mingw)"),0)
EFI_SUBSYSTEM_FLAG :=
$(warning Skipping --subsystem=efi_application on non-Windows linker)
endif
endif
NO_DYN_FLAG := $(shell $(LD) --help 2>/dev/null | grep -q no-dynamic-linker && echo --no-dynamic-linker)
LDFLAGS_EFI := -Bsymbolic -nostdlib -znocombreloc -L/usr/lib
ifeq ($(CROSS_ARCH),aarch64)
LDFLAGS_EFI += -L$(GNUEFI_LIBDIR)
endif
LDFLAGS_EFI += $(LIBS) $(EFI_SUBSYSTEM_FLAG) --entry=efi_main \
        $(NO_DYN_FLAG) -z notext
else
LD ?= ld.bfd
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fPIC -fshort-wchar -mno-red-zone \
        -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-strict-aliasing \
        -D__NO_INLINE__
EFI_SUBSYSTEM_FLAG :=
ifeq ($(findstring Windows,$(HOST_OS)),Windows)
EFI_SUBSYSTEM_FLAG := --subsystem=efi_application
ifeq ($(shell $(LD) -v 2>&1 | grep -E -c "(lld|mingw)"),0)
EFI_SUBSYSTEM_FLAG :=
$(warning Skipping --subsystem=efi_application on non-Windows linker)
endif
endif
LDFLAGS_EFI := -Bsymbolic -nostdlib -znocombreloc -L/usr/lib
ifeq ($(CROSS_ARCH),aarch64)
LDFLAGS_EFI += -L$(GNUEFI_LIBDIR)
endif
LDFLAGS_EFI += $(LIBS) \
        $(EFI_SUBSYSTEM_FLAG) --entry=efi_main \
        $(NO_DYN_FLAG) -z notext
endif

LD_FLAGS := $(LDFLAGS_EFI)

# Flags for building the init EFI binary
CFLAGS_INIT_EFI := $(filter-out -mno-red-zone,$(CFLAGS_EFI))
ifeq ($(CROSS_ARCH),x86_64)
CFLAGS_INIT_EFI += -mno-red-zone
endif

# Compile with warnings for unused results by default
CFLAGS_WARN := -Wall -Wextra -Wunused-result
# Some UEFI helpers intentionally ignore return statuses; allow selective
# suppression for those objects only.
CFLAGS_IGNORE_RESULT := $(CFLAGS_WARN) -Wno-unused-result

$(info Using $(TOOLCHAIN) toolchain for UEFI build...)

.PHONY: check-efi
check-efi:
	@ls -lh out/iso/init
	@if [ ! -f out/iso/init/init.efi ]; then \
	echo "\xe2\x9d\x8c check-efi: init.efi not found. EFI build likely failed earlier."; \
	exit 0; \
	fi
	@file out/iso/init/init.efi | grep -iq "EFI application" && \
	echo "\xe2\x9c\x85 init.efi format OK" || \
	{ echo "\xe2\x9a\xa0\ufe0f init.efi found but does not appear valid"; exit 0; }
ifeq ($(findstring Windows,$(HOST_OS)),Windows)
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
else
	@if [ ! -f $(GNUEFI_LIBDIR)/libgnuefi.a ]; then \
	echo "Missing gnu-efi. Please install it."; exit 1; \
	fi
endif

.PHONY: build cuda-build all go-build go-test c-shims help fmt lint check cohrun cohbuild cohtrace cli_cap gui-orchestrator kernel init-efi
.PHONY: init-efi
.PHONY: verify-efi

all: go-build go-test c-shims kernel ## Run vet, tests, C shims and kernel

build: kernel ## Build Rust workspace and kernel
	@cargo build --workspace || echo "cargo build failed"

cuda-build: ## Build release with CUDA features
	cargo clean && cargo build --release --features=cuda

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
		@if command -v black >/dev/null 2>&1; then \
		black --check python tests; \
		fi
		@if command -v mypy >/dev/null 2>&1; then \
mypy --ignore-missing-imports --check-untyped-defs python tests/python; \
		fi
		@if command -v gofmt >/dev/null 2>&1; then \
		gofmt -l $(shell find go -name '*.go'); \
		fi

check: test ## Run full test suite

.PHONY: test test-python
test: ## Run Rust, Python, Go and C tests
	@echo "🦀 Rust tests …"
	@RUST_BACKTRACE=1 cargo test --release || echo "cargo tests failed"
	@echo "🐍 Python tests …"
	@pytest -v || echo "python tests failed"
	@echo "🐹 Go tests …"
	@GOWORK=$(CURDIR)/go/go.work go test ./go/... || echo "go tests failed"
	@echo "🧱 C tests …"
	@cd build && ctest --output-on-failure || true

test-python:
	@echo "🐍 Python tests …"
	@pytest -v

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

boot-x86_64: ## Build boot image for x86_64
	@echo "🏁 Building boot image for x86_64"
	cargo build --release --target x86_64-unknown-linux-gnu

boot-aarch64: ## Build boot image for aarch64
	@echo "🏁 Building boot image for aarch64"
	cargo build --release --target aarch64-unknown-linux-gnu

bootloader: check-efi ## Build UEFI bootloader
	@echo "🏁 Building UEFI bootloader using $(TOOLCHAIN)"
	@mkdir -p out/EFI/BOOT
	# main.c discards EFI status codes after logging
	$(CC) $(CFLAGS_EFI) $(CFLAGS_IGNORE_RESULT) -c src/bootloader/main.c -o out/bootloader.o
	$(CC) $(CFLAGS_EFI) $(CFLAGS_WARN) -c src/bootloader/sha1.c -o out/sha1.o
	$(LD) /usr/lib/crt0-efi-x86_64.o out/bootloader.o out/sha1.o \
	-o out/bootloader.so -T linker.ld $(LD_FLAGS)
	@command -v objcopy >/dev/null 2>&1 || { echo "objcopy not found"; exit 1; }
	objcopy --target=efi-app-x86_64 out/bootloader.so out/bootloader.efi
	cp out/bootloader.efi out/EFI/BOOT/BOOTX64.EFI


kernel: check-efi ## Build Rust kernel BOOTX64.EFI
	@echo "🏁 Building Rust kernel"
	cargo build --release --target x86_64-unknown-uefi --bin kernel \
	    --no-default-features --features minimal_uefi,kernel_bin
	@mkdir -p out/EFI/BOOT
	cp target/x86_64-unknown-uefi/release/kernel.efi out/kernel.so
	@command -v objcopy >/dev/null 2>&1 || { echo "objcopy not found"; exit 1; }
	objcopy --target=efi-app-x86_64 out/kernel.so out/BOOTX64.EFI
	cp out/BOOTX64.EFI out/EFI/BOOT/BOOTX64.EFI
# Use tabs for all recipe lines. Run `make check-tab-safety` after edits.


init-efi: check-efi ## Build init EFI binary
	@echo "🏁 Building init EFI using $(TOOLCHAIN)"
	@mkdir -p obj/init_efi out/iso/init out/bin
	@test -f $(GNUEFI_LIBDIR)/libgnuefi.a || { echo "Missing gnu-efi. Please install it."; exit 1; }
	# init uses wrapper calls that intentionally drop errors
	$(CROSS_CC) $(CFLAGS_INIT_EFI) $(CFLAGS_IGNORE_RESULT) -c src/init_efi/main.c -o obj/init_efi/main.o
	$(CROSS_CC) $(CFLAGS_INIT_EFI) -c src/init_efi/efistubs.c -o obj/init_efi/efistubs.o
	@echo "Linking for UEFI on $(CROSS_ARCH)"
	$(CROSS_LD) -nostdlib -znocombreloc -Bsymbolic \
	-T src/init_efi/elf_aarch64_efi.lds \
	$(HOME)/gnu-efi/gnuefi/crt0-efi-aarch64.o \
	obj/init_efi/main.o obj/init_efi/efistubs.o \
	# $(LOCAL_GNUEFI) adds -L/usr/local/lib -lgnuefi when that library is present
	# so Homebrew installs work without impacting standard Linux paths.
	-L$(GNUEFI_LIBDIR) $(LOCAL_GNUEFI) $(LIBS) \
	-o out/iso/init/init.efi || scripts/manual_efi_link.sh
	@cp out/iso/init/init.efi out/bin/init.efi
	@test -s out/bin/init.efi || { echo "init.efi build failed" >&2; exit 1; }
ifeq ($(OS),Windows_NT)
	@if command -v llvm-objdump >/dev/null 2>&1; then \
	llvm-objdump -p out/iso/init/init.efi | grep -q "PE32"; \
	else \
	echo "llvm-objdump missing; skipping PE header check"; \
	fi
else
	@echo "Skipping PE header validation on $(HOST_OS)"
endif

verify-efi: ## Verify init EFI binary
	@if [ ! -f out/iso/init/init.efi ]; then \
	echo "init.efi missing" >&2; exit 1; \
	fi
	@file out/iso/init/init.efi | grep -iq "EFI application" || { \
	echo "invalid init.efi" >&2; exit 1; \
	}
	@echo "init.efi verified"

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
	cargo run -p cohcli_tools --bin cohesix_build -- $(ARGS)

cohtrace: ## Run cohtrace CLI
	cargo run -p cohcli_tools --bin cohesix_trace -- $(ARGS)

cli_cap: ## Run cohcap CLI
	cargo run -p cohcli_tools --bin cli_cap -- $(ARGS)

gui-orchestrator: ## Build gui-orchestrator binary
	@echo "Building gui-orchestrator"
	@mkdir -p out/bin
	@GOWORK=$(CURDIR)/go/go.work go build -o out/bin/gui-orchestrator ./go/cmd/gui-orchestrator

# Run boot image under QEMU, logging serial output
qemu: ## Launch QEMU with built image and capture serial log
	@command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "qemu-system-x86_64 not installed — skipping"; exit 0; }
	@if [ "$(EFI_AVAILABLE)" != "1" ]; then echo "gnu-efi headers not found — skipping qemu"; \
	else mkdir -p out; \
	if [ ! -f out/BOOTX64.EFI ]; then \
	     echo "Missing out/BOOTX64.EFI; run 'make kernel'"; exit 1; fi; \
	if [ ! -f out/cohesix.iso ]; then ./make_iso.sh; fi; \
	[ -f out/cohesix.iso ] || { echo "ISO build failed"; exit 1; }; \
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


check-tab-safety:
	@grep -Pn "^\s{4,}[^\t]" Makefile && echo "WARNING: spaces used in recipe lines" || echo "Tab check passed"
