cmake_minimum_required(VERSION 3.16.0)

add_custom_target(kernel_config_target)

if(NOT KernelSel4Arch)
    message(FATAL_ERROR "KernelSel4Arch not set")
endif()

include(${CMAKE_CURRENT_LIST_DIR}/src/arch/${KernelSel4Arch}/config.cmake)
