# Author: Lukas Bower
# Purpose: Configure Cohesix seL4 profiles from a complete pinned upstream project checkout.
# Copyright 2026 Lukas Bower

if(COHESIX_GENERATE_ELFLOADER_PLATFORM_INFO)
    foreach(
        _required
        IN ITEMS
            COHESIX_PYTHON3
            COHESIX_PLATFORM_SIFT
            COHESIX_PLATFORM_YAML
            COHESIX_PLATFORM_INFO
    )
        if(NOT DEFINED ${_required} OR "${${_required}}" STREQUAL "")
            message(FATAL_ERROR "${_required} is required to generate platform_info.h")
        endif()
    endforeach()

    set(_cohesix_platform_info_tmp "${COHESIX_PLATFORM_INFO}.tmp")
    execute_process(
        COMMAND
            "${COHESIX_PYTHON3}"
            "${COHESIX_PLATFORM_SIFT}"
            --emit-c-syntax
            "${COHESIX_PLATFORM_YAML}"
        OUTPUT_FILE "${_cohesix_platform_info_tmp}"
        ERROR_VARIABLE _cohesix_platform_sift_error
        RESULT_VARIABLE _cohesix_platform_sift_result
    )
    if(NOT _cohesix_platform_sift_result EQUAL 0)
        file(REMOVE "${_cohesix_platform_info_tmp}")
        message(
            FATAL_ERROR
            "platform_sift.py failed (${_cohesix_platform_sift_result}): ${_cohesix_platform_sift_error}"
        )
    endif()
    file(READ "${_cohesix_platform_info_tmp}" _cohesix_platform_info_text)
    if(NOT _cohesix_platform_info_text MATCHES "memory_region")
        file(REMOVE "${_cohesix_platform_info_tmp}")
        message(FATAL_ERROR "generated platform_info.h has no memory_region declaration")
    endif()
    file(RENAME "${_cohesix_platform_info_tmp}" "${COHESIX_PLATFORM_INFO}")
    return()
endif()

cmake_minimum_required(VERSION 3.16.0)

file(SHA256 "${CMAKE_CURRENT_LIST_FILE}" _cohesix_sel4_wrapper_sha256)
set(
    COHESIX_SEL4_WRAPPER_SHA256
    "${_cohesix_sel4_wrapper_sha256}"
    CACHE INTERNAL
    "SHA-256 of the Cohesix seL4 profile wrapper used at configure time"
)

set(
    COHESIX_SEL4_PROJECT_ROOT
    ""
    CACHE PATH
    "Complete external seL4 project checkout pinned by configs/sel4/profiles.toml"
)

if(NOT IS_ABSOLUTE "${COHESIX_SEL4_PROJECT_ROOT}")
    message(FATAL_ERROR "COHESIX_SEL4_PROJECT_ROOT must be an absolute path")
endif()

if(DEFINED SEL4_CACHE_DIR AND NOT "${SEL4_CACHE_DIR}" STREQUAL "")
    message(
        FATAL_ERROR
        "Canonical Cohesix seL4 profiles forbid the unbound binary memoization cache"
    )
endif()
set(SEL4_CACHE_DIR "" CACHE PATH "seL4 binary memoization is disabled" FORCE)
set(ENV{SEL4_CACHE_DIR} "")

set(_cohesix_sel4test_dir "${COHESIX_SEL4_PROJECT_ROOT}/projects/sel4test")
if(NOT EXISTS "${_cohesix_sel4test_dir}/settings.cmake")
    message(
        FATAL_ERROR
        "Pinned seL4 project checkout is missing projects/sel4test/settings.cmake: ${COHESIX_SEL4_PROJECT_ROOT}"
    )
endif()

include("${_cohesix_sel4test_dir}/settings.cmake")

project(cohesix-sel4-profile C CXX ASM)

find_package(seL4 REQUIRED)
find_package(elfloader-tool REQUIRED)

set(KernelRootCNodeSizeBits 13 CACHE INTERNAL "")
sel4_import_kernel()

if((NOT Sel4testAllowSettingsOverride) AND (KernelArchARM OR KernelArchRiscV))
    ApplyData61ElfLoaderSettings(${KernelPlatform} ${KernelSel4Arch})
endif()
elfloader_import_project()

# Upstream uses a fixed image address for bcm2711 and emits an intentionally
# empty platform_info.h in that mode. Cohesix also loads rootservers last, whose
# elfloader path requires the generated physical-memory map. Recreate the same
# platform_sift step used by the canonical Pi image build before elfloader
# compilation, without modifying the pinned upstream checkout.
if(ElfloaderRootserversLast AND NOT "${IMAGE_START_ADDR}" STREQUAL "")
    set(
        _cohesix_platform_sift
        "${COHESIX_SEL4_PROJECT_ROOT}/tools/seL4/cmake-tool/helpers/platform_sift.py"
    )
    set(
        _cohesix_platform_info
        "${CMAKE_CURRENT_BINARY_DIR}/elfloader/gen_headers/platform_info.h"
    )
    if(NOT DEFINED platform_yaml OR "${platform_yaml}" STREQUAL "")
        message(
            FATAL_ERROR
            "ElfloaderRootserversLast with IMAGE_START_ADDR requires kernel platform_yaml"
        )
    endif()
    add_custom_target(
        cohesix_elfloader_platform_info
        COMMAND
            "${CMAKE_COMMAND}"
            "-DCOHESIX_GENERATE_ELFLOADER_PLATFORM_INFO=ON"
            "-DCOHESIX_PYTHON3=${PYTHON3}"
            "-DCOHESIX_PLATFORM_SIFT=${_cohesix_platform_sift}"
            "-DCOHESIX_PLATFORM_YAML=${platform_yaml}"
            "-DCOHESIX_PLATFORM_INFO=${_cohesix_platform_info}"
            -P
            "${CMAKE_CURRENT_LIST_FILE}"
        BYPRODUCTS "${_cohesix_platform_info}"
        DEPENDS
            "${platform_yaml}"
            "${_cohesix_platform_sift}"
            "${CMAKE_CURRENT_LIST_FILE}"
        VERBATIM
    )
    add_dependencies(elfloader cohesix_elfloader_platform_info)
endif()

add_subdirectory(
    "${_cohesix_sel4test_dir}/apps/sel4test-driver"
    "${CMAKE_CURRENT_BINARY_DIR}/apps/sel4test-driver"
)

# The operational QEMU build replaces the wrapper rootserver inside the
# elfloader CPIO without mutating the validated seL4 tree. Reserve explicit,
# contract-bound file space in that non-shipping placeholder so a larger
# Cohesix root task can be substituted without weakening archive bounds.
set(
    COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES
    "0"
    CACHE STRING
    "Extra file-backed bytes reserved in the wrapper rootserver ELF"
)
if(NOT COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES MATCHES "^[0-9]+$")
    message(FATAL_ERROR "COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES must be decimal bytes")
endif()
if(COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES GREATER 0)
    set(
        _cohesix_rootserver_reserve_source
        "${CMAKE_CURRENT_BINARY_DIR}/cohesix_rootserver_archive_reserve.c"
    )
    file(
        WRITE
        "${_cohesix_rootserver_reserve_source}"
        "/* Author: Lukas Bower */\n"
        "/* Purpose: Reserve validated elfloader archive capacity for the Cohesix root task. */\n"
        "/* Copyright 2026 Lukas Bower */\n"
        "__attribute__((used, section(\".rodata.cohesix_archive_reserve\")))\n"
        "const unsigned char cohesix_rootserver_archive_reserve[${COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES}] = {0x43};\n"
    )
    target_sources(sel4test-driver PRIVATE "${_cohesix_rootserver_reserve_source}")
endif()
