# CLASSIFICATION: COMMUNITY
# Filename: build_config.cmake v0.1
# Author: Lukas Bower
# Date Modified: 2027-12-31

set(PLATFORM "qemu_arm_virt" CACHE STRING "")
set(KernelArch "aarch64" CACHE STRING "")
set(KernelWordSize 64 CACHE STRING "")
set(KernelSel4Arch "aarch64" CACHE STRING "")
set(CROSS_COMPILER_PREFIX "aarch64-linux-gnu-" CACHE STRING "")
