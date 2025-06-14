# CLASSIFICATION: COMMUNITY
# Filename: CMakeLists.txt v0.4
# Author: Lukas Bower
# Date Modified: 2025-07-22

cmake_minimum_required(VERSION 3.14)
cmake_policy(SET CMP0146 NEW)
project(CohesixNative LANGUAGES C CXX)

option(ENABLE_TESTS "Build C tests" ON)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Optional CUDA support
find_package(CUDAToolkit)
if(CUDAToolkit_FOUND)
    if(DEFINED ENV{TMPDIR} AND NOT "$ENV{TMPDIR}" STREQUAL "")
        set(cuda_tmp "$ENV{TMPDIR}/cohesix_cuda")
    else()
        set(cuda_tmp "/tmp/cohesix_cuda")
    endif()
    file(MAKE_DIRECTORY "${cuda_tmp}")
    set(CMAKE_CUDA_FLAGS "--keep-dir=${cuda_tmp}")
    enable_language(CUDA)
endif()

file(GLOB SRC_C src/c/*.c)
if(NOT SRC_C)
    set(SRC_C src/c/placeholder.c)
endif()

add_library(cohesix_c STATIC ${SRC_C})
target_include_directories(cohesix_c PUBLIC src/c)

if(ENABLE_TESTS)
    enable_testing()
    add_executable(c_tests test/test_c_main.c)
    target_link_libraries(c_tests PRIVATE cohesix_c)
    add_test(NAME c-tests COMMAND c_tests)
endif()
