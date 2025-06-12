# CLASSIFICATION: COMMUNITY
# Filename: CMakeLists.txt v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22

cmake_minimum_required(VERSION 3.14)
project(CohesixNative LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Optional CUDA support
find_package(CUDA)
if(CUDA_FOUND)
    enable_language(CUDA)
endif()

# Future Rust integration via cxx crate

add_library(hello STATIC native/hello.cpp)
target_include_directories(hello PUBLIC native)
