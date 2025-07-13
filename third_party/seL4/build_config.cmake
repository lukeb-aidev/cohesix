# CLASSIFICATION: COMMUNITY
# Filename: build_config.cmake v0.2
# Author: Lukas Bower
# Date Modified: 2027-12-31

set(PLATFORM "qemu-arm-virt" CACHE STRING "")
set(KernelArch "ARM" CACHE STRING "")
set(KernelArchArmV8a ON CACHE BOOL "")
set(KernelSel4Arch "aarch64" CACHE STRING "")
set(KernelSel4ArchAarch64 ON CACHE BOOL "")
set(KernelSel4ArchArmHyp OFF CACHE BOOL "")
set(KernelWordSize 64 CACHE STRING "")
set(AARCH64 ON CACHE BOOL "")
set(SIMULATION ON CACHE BOOL "")
set(RELEASE ON CACHE BOOL "")
set(CROSS_COMPILER_PREFIX "aarch64-linux-gnu-" CACHE STRING "")