// CLASSIFICATION: COMMUNITY
// Filename: COHESIX_AARCH64_BUILD.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-11

# 📜 Canonical Build Guide for Cohesix on seL4 AArch64

This document consolidates the best practices and research into a detailed, structured process for building and running the Cohesix rootserver on a clean release-mode seL4 kernel for AArch64 using the elfloader. It leverages official seL4 methodologies and integrates Cohesix specifics.

---

## 🚀 Objectives

- Build a **release-mode seL4 kernel** targeting AArch64 (`qemu-arm-virt` platform).
- Use the official **elfloader flow** with a CPIO archive to load the kernel and Cohesix rootserver.
- Ensure **no sel4test or test harness code** executes, only the clean Cohesix ELF.

---

## 🔧 Prerequisites

- Ubuntu (or similar) build environment.
- Cross compiler for AArch64:
  ```bash
  sudo apt install gcc-aarch64-linux-gnu
  ```
- CMake & Ninja:
  ```bash
  sudo apt install cmake ninja-build cpio repo
  ```
- QEMU for AArch64:
  ```bash
  sudo apt install qemu-system-arm
  ```

---

## 📂 Directory Structure

We’ll use the following structure:

```
~/cohesix-seL4/        # seL4 kernel + elfloader source
~/cohesix/out/         # your built cohesix_root.elf
~/cohesix-cpio/        # CPIO archive build dir
```

---

## ✅ Step 1: Clone the official seL4 repo

Use the standard multi-arch seL4 with QEMU platform support:

```bash
mkdir -p ~/cohesix-seL4
cd ~/cohesix-seL4
repo init -u https://github.com/seL4/sel4test-manifest.git
repo sync
```

> Even though this uses `sel4test-manifest` for fetching complete sources (plat/arch overlays), we will override to **skip all test harnesses**.

---

## ⚙️ Step 2: Configure for AArch64 release build

```bash
mkdir -p ~/cohesix-seL4/build
cd ~/cohesix-seL4/build

../init-build.sh \
  -DPLATFORM=qemu-arm-virt \
  -DAARCH64=1 \
  -DRELEASE=1 \
  -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu-
```

This configures:
- AArch64 target on the `virt` platform.
- Release mode (optimizations, no debug/test symbols).
- Uses your cross compiler.

---

## 🛠️ Step 3: Build kernel & elfloader

```bash
ninja kernel.elf elfloader
```

This produces:
```
~/cohesix-seL4/build/kernel/kernel.elf
~/cohesix-seL4/build/elfloader/elfloader
```

---

## 📦 Step 4: Prepare the CPIO archive

Use the standard elfloader packaging strategy:

```bash
mkdir -p ~/cohesix-cpio
cd ~/cohesix-cpio

cp ~/cohesix-seL4/build/kernel/kernel.elf ./
cp ~/cohesix/out/cohesix_root.elf ./

# (Optional) if your board needs a DTB
# cp ~/cohesix-seL4/build/qemu-arm-virt.dtb ./

# Build the archive
find . | cpio -o -H newc > ../image.cpio
```

---

## 🚀 Step 5: Run in QEMU

Use `elfloader` with your CPIO archive:

```bash
cd ~/cohesix-seL4/build

qemu-system-aarch64 -M virt -cpu cortex-a57 -m 1024 \
  -kernel elfloader/elfloader \
  -initrd ~/image.cpio \
  -nographic -serial mon:stdio
```

---

## 📝 Verification checklist

✅ Ensure your QEMU boot log **does NOT mention `sel4test`**. It should instead log your Cohesix rootserver startup sequence.

✅ Check for correct entry point and seL4 MMU setup logs (IOPT levels, IPC buffer, untypeds) without triggering test suite patterns.

---

## ⚡ Notes for advanced setups

- You can force explicit `ENTRY(_start)` in your Cohesix linker script to guarantee the ELF entry point.
- Use `-DCMAKE_TOOLCHAIN_FILE` if you prefer explicit CMake toolchains.
- Use `-C ../configs/ARM_verified.cmake` for verified kernel profiles if needed.

---

## ✅ Summary: minimal canonical process

| Phase   | Tool    | Description                                     |
|---------|---------|-------------------------------------------------|
| Setup   | repo    | Clone multi-arch seL4 sources                   |
| Build   | CMake + Ninja | Configure release AArch64 kernel + elfloader |
| Package | cpio    | Build archive with kernel & Cohesix ELF         |
| Run     | QEMU    | Boot via elfloader with image.cpio              |

---

✅ **Done.**

This is the canonical community-aligned process for running Cohesix on seL4 AArch64 with no test overhead, built from official literature and platform guidelines. 🚀