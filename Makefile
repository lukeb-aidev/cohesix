# CLASSIFICATION: COMMUNITY
# Filename: Makefile v0.12
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

.PHONY: all go-build go-test c-shims help boot boot-x86_64 boot-aarch64 bootloader kernel

PLATFORM ?= $(shell uname -m)

# Detect compiler; use environment override if provided
CC ?= $(shell command -v clang >/dev/null 2>&1 && echo clang || echo gcc)
TOOLCHAIN := $(if $(findstring clang,$(CC)),clang,gcc)

# Ensure compiler exists
ifeq ($(shell command -v $(CC) >/dev/null 2>&1 && echo yes || echo no),no)
$(error Compiler $(CC) not found)
endif

EFI_INCLUDES := -I/usr/include/efi -I/usr/include/efi/x86_64

ifeq ($(TOOLCHAIN),clang)
LD ?= ld.lld
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fshort-wchar -mno-red-zone \
       -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-pie \
       -target x86_64-pc-win32-coff
LDFLAGS_EFI := -shared -Bsymbolic -nostdlib -znocombreloc -L/usr/lib \
       -lgnuefi -lefi -Wl,--subsystem,efi_application -Wl,--entry,efi_main
else
LD ?= ld.bfd
CFLAGS_EFI := $(EFI_INCLUDES) -ffreestanding -fPIC -fshort-wchar -mno-red-zone \
       -DEFI_FUNCTION_WRAPPER -DGNU_EFI -fno-stack-protector -fno-strict-aliasing \
       -D__NO_INLINE__
LDFLAGS_EFI := -shared -Bsymbolic -nostdlib -znocombreloc -L/usr/lib -lgnuefi -lefi
endif

LD_FLAGS := $(LDFLAGS_EFI)

$(info Using $(TOOLCHAIN) toolchain for UEFI build...)

.PHONY: all go-build go-test c-shims help cohrun cohbuild cohtrace cohcap kernel

all: go-build go-test c-shims

.PHONY: test
test:
	@echo "🦀 Rust tests …"
	@RUST_BACKTRACE=1 cargo test --release
	@echo "🐍 Python tests …"
	@pytest -v
	@echo "🐹 Go tests …"
	@go test ./...
	@echo "🧱 C tests …"
	@cd build && ctest --output-on-failure || true

go-build:
	@echo "🔧 Go vet …"
	@cd go && go vet ./...

go-test:
	@echo "🔧 Go unit tests …"
	@GOWORK=$(CURDIR)/go/go.work go test ./go/...

c/sel4/shim/boot_trampoline.o: c/sel4/shim/boot_trampoline.c
	$(CC) -I c/sel4/include -c $< -o $@

c/sel4/bootloader.o: c/sel4/bootloader.c
	$(CC) -I c/sel4/include -c $< -o $@

c-shims: c/sel4/shim/boot_trampoline.o c/sel4/bootloader.o
	@echo "🔧 Building C shims …"

boot-x86_64:
	@echo "🏁 Building boot image for x86_64"
	cargo build --release --target x86_64-unknown-linux-gnu

boot-aarch64:
	@echo "🏁 Building boot image for aarch64"
	cargo build --release --target aarch64-unknown-linux-gnu

bootloader:
	@echo "🏁 Building UEFI bootloader using $(TOOLCHAIN)"
	@mkdir -p out/EFI/BOOT
	$(CC) $(CFLAGS_EFI) -c src/bootloader/main.c -o out/bootloader.o
	grep -v '^//' bootloader.lds > out/bootloader.tmp.ld
	$(LD) /usr/lib/crt0-efi-x86_64.o out/bootloader.o -o out/bootloader.so \
	-T out/bootloader.tmp.ld $(LD_FLAGS)
	rm -f out/bootloader.tmp.ld
	objcopy --target=efi-app-x86_64 out/bootloader.so out/BOOTX64.EFI
	cp out/BOOTX64.EFI out/EFI/BOOT/BOOTX64.EFI


kernel:
	@echo "🏁 Building kernel stub using $(TOOLCHAIN)"
	@mkdir -p out
	$(CC) $(CFLAGS_EFI) -c src/kernel/main.c -o out/kernel.o
	grep -v '^//' linker.ld > out/kernel.tmp.ld
	$(LD) /usr/lib/crt0-efi-x86_64.o out/kernel.o -o out/kernel.so \
	-T out/kernel.tmp.ld $(LD_FLAGS)
	rm -f out/kernel.tmp.ld
	objcopy --target=efi-app-x86_64 out/kernel.so out/kernel.elf

boot:
	$(MAKE) boot-$(PLATFORM)


	testboot:
	./test_boot_efi.sh

	print-env:
	@echo "Toolchain: $(TOOLCHAIN)"
	@echo "Compiler: $(CC)"
	@$(CC) --version | head -n 1

help:
	@echo "Cohesix top‑level build targets:"
	@echo "  all       – run go-build, go-test and c-shims"
	@echo "  go-build  – vet Go workspace"
	@echo "  go-test   – run Go unit tests"
	@echo "  c-shims   – compile seL4 boot trampoline"
	@echo "  testboot  – run UEFI boot test via QEMU"
	@echo "  print-env – display selected compiler information"
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



