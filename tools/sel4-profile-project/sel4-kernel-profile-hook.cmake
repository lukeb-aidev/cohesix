# Author: Lukas Bower
# Purpose: Bind the QEMU seL4 kernel timer contract to the selected virtual counter frequency.
# Copyright 2026 Lukas Bower

# CMake loads this file through CMAKE_PROJECT_seL4_INCLUDE immediately after
# project(seL4), once upstream platform selection has populated configure_string
# but before kernel/config.cmake emits configuration headers.
if(NOT "${KernelPlatform}" STREQUAL "qemu-arm-virt")
    message(FATAL_ERROR "Cohesix QEMU timer hook requires KernelPlatform=qemu-arm-virt")
endif()
if(NOT KernelSel4ArchAarch64 OR NOT KernelIsMCS)
    message(FATAL_ERROR "Cohesix QEMU timer hook requires the AArch64 MCS kernel")
endif()
if(
    NOT DEFINED COHESIX_TIMER_CLOCK_HZ
    OR NOT "${COHESIX_TIMER_CLOCK_HZ}" MATCHES "^[1-9][0-9]*$"
)
    message(FATAL_ERROR "COHESIX_TIMER_CLOCK_HZ must be a positive decimal frequency")
endif()

string(
    REGEX MATCHALL
    "TIMER_FREQUENCY: \"[0-9]+\""
    _cohesix_timer_entries
    "${configure_string}"
)
list(LENGTH _cohesix_timer_entries _cohesix_timer_entry_count)
if(NOT _cohesix_timer_entry_count EQUAL 1)
    message(
        FATAL_ERROR
        "Expected exactly one upstream TIMER_FREQUENCY entry, found ${_cohesix_timer_entry_count}"
    )
endif()

set(
    COHESIX_UPSTREAM_TIMER_CLOCK_HZ
    "${CONFIGURE_TIMER_FREQUENCY}"
    CACHE INTERNAL
    "Upstream QEMU timer frequency replaced by the Cohesix execution profile"
    FORCE
)
string(
    REGEX REPLACE
    "TIMER_FREQUENCY: \"[0-9]+\""
    "TIMER_FREQUENCY: \"${COHESIX_TIMER_CLOCK_HZ}\""
    configure_string
    "${configure_string}"
)
set(CONFIGURE_TIMER_FREQUENCY "${COHESIX_TIMER_CLOCK_HZ}")
set(
    KernelTimerFrequency
    "${COHESIX_TIMER_CLOCK_HZ}"
    CACHE INTERNAL
    "Timer frequency selected by the Cohesix QEMU execution profile"
    FORCE
)
