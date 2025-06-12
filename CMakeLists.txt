# CLASSIFICATION: COMMUNITY
# Filename: CMakeLists.txt v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-22

cmake_minimum_required(VERSION 3.14)
cmake_policy(SET CMP0146 NEW)
project(CohesixNative LANGUAGES C CXX)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Optional CUDA support
find_package(CUDAToolkit)
if(CUDAToolkit_FOUND)
    enable_language(CUDA)
endif()

file(GLOB SRC_C src/c/*.c)
if(NOT SRC_C)
    set(SRC_C src/c/placeholder.c)
endif()

add_library(cohesix_c STATIC ${SRC_C})
target_include_directories(cohesix_c PUBLIC src/c)
