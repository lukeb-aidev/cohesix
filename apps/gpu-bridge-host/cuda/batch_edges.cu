// Author: Lukas Bower
// Purpose: Run a bounded batch Sobel edge extraction recipe on the enrolled CUDA device.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

#include <cuda_runtime.h>
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>
#include <sys/prctl.h>
#include <unistd.h>

namespace {
constexpr size_t kHeadroom = size_t{2} << 30;
constexpr size_t kMaximumPixels = 262144;

bool number(const char* value, unsigned maximum, unsigned* output) {
    if (!value || !*value) return false;
    for (const char* cursor = value; *cursor; ++cursor) {
        if (*cursor < '0' || *cursor > '9') return false;
    }
    errno = 0;
    char* end = nullptr;
    unsigned long parsed = std::strtoul(value, &end, 10);
    if (errno || *end || parsed == 0 || parsed > maximum) return false;
    *output = static_cast<unsigned>(parsed);
    return true;
}

int failure(const char* code, cudaError_t error = cudaSuccess) {
    std::fprintf(stderr, "{\"code\":\"%s\",\"cuda_error\":%d}\n", code, static_cast<int>(error));
    return error == cudaErrorMemoryAllocation ? 5 : 2;
}

struct DeviceBuffer {
    unsigned char* value = nullptr;
    ~DeviceBuffer() { if (value) cudaFree(value); }
};

__global__ void edges(const unsigned char* input, unsigned char* output,
                      unsigned width, unsigned height, unsigned frames) {
    size_t index = size_t{blockIdx.x} * blockDim.x + threadIdx.x;
    size_t plane = size_t{width} * height;
    if (index >= plane * frames) return;
    unsigned x = static_cast<unsigned>(index % width);
    unsigned y = static_cast<unsigned>((index / width) % height);
    if (x == 0 || y == 0 || x + 1 == width || y + 1 == height) {
        output[index] = 0;
        return;
    }
    size_t row = index - x;
    int tl = input[row - width + x - 1];
    int tc = input[row - width + x];
    int tr = input[row - width + x + 1];
    int ml = input[row + x - 1];
    int mr = input[row + x + 1];
    int bl = input[row + width + x - 1];
    int bc = input[row + width + x];
    int br = input[row + width + x + 1];
    int gx = -tl + tr - 2 * ml + 2 * mr - bl + br;
    int gy = -tl - 2 * tc - tr + bl + 2 * bc + br;
    int magnitude = abs(gx) + abs(gy);
    output[index] = static_cast<unsigned char>(magnitude > 255 ? 255 : magnitude);
}
}

#define CUDA_CHECK(operation) do { cudaError_t error = (operation); \
    if (error != cudaSuccess) return failure("cuda_failure", error); } while (false)

int main(int argc, char** argv) {
    unsigned owner = 0;
    if (!number(std::getenv("COH_REFERENCE_OWNER_PID"), 2147483647, &owner)
        || static_cast<unsigned>(getppid()) != owner
        || prctl(PR_SET_PDEATHSIG, SIGKILL) != 0
        || static_cast<unsigned>(getppid()) != owner) return failure("owner_binding");
    if (argc != 13 || std::strcmp(argv[1], "run") != 0
        || std::strcmp(argv[3], "input.bin") != 0
        || std::strcmp(argv[4], "output.bin") != 0) return failure("invalid_request");
    unsigned width = 0, height = 0, frames = 0, iterations = 0;
    bool seen_width = false, seen_height = false, seen_frames = false, seen_iterations = false;
    for (int i = 5; i < argc; i += 2) {
        if (std::strcmp(argv[i], "width") == 0 && !seen_width) {
            seen_width = number(argv[i + 1], 256, &width);
        } else if (std::strcmp(argv[i], "height") == 0 && !seen_height) {
            seen_height = number(argv[i + 1], 256, &height);
        } else if (std::strcmp(argv[i], "frames") == 0 && !seen_frames) {
            seen_frames = number(argv[i + 1], 4, &frames);
        } else if (std::strcmp(argv[i], "iterations") == 0 && !seen_iterations) {
            seen_iterations = number(argv[i + 1], 100000, &iterations);
        } else return failure("invalid_parameter");
    }
    if (!seen_width || !seen_height || !seen_frames || !seen_iterations
        || width < 3 || height < 3)
        return failure("invalid_bound");
    size_t pixels = size_t{width} * height * frames;
    if (pixels > kMaximumPixels) return failure("invalid_bound");
    CUDA_CHECK(cudaSetDevice(0));
    cudaDeviceProp properties{};
    CUDA_CHECK(cudaGetDeviceProperties(&properties, 0));
    char uuid[33]{};
    for (unsigned i = 0; i < 16; ++i) {
        std::snprintf(uuid + i * 2, 3, "%02x", static_cast<unsigned char>(properties.uuid.bytes[i]));
    }
    if (std::strcmp(uuid, argv[2]) != 0) return failure("wrong_device");
    int runtime = 0, driver = 0;
    CUDA_CHECK(cudaRuntimeGetVersion(&runtime));
    CUDA_CHECK(cudaDriverGetVersion(&driver));
    if (runtime != 13020 || driver < 13020) return failure("profile_mismatch");
    size_t free_bytes = 0, total_bytes = 0;
    CUDA_CHECK(cudaMemGetInfo(&free_bytes, &total_bytes));
    if (free_bytes < kHeadroom || pixels * 2 > free_bytes - kHeadroom)
        return failure("memory_admission");
    std::vector<unsigned char> input(pixels), output(pixels);
    FILE* source = std::fopen("input.bin", "rb");
    if (!source) return failure("input_unavailable");
    size_t loaded = std::fread(input.data(), 1, pixels, source);
    int extra = std::fgetc(source);
    int source_close = std::fclose(source);
    if (loaded != pixels || extra != EOF || source_close != 0)
        return failure("input_size");
    DeviceBuffer device_input, device_output;
    CUDA_CHECK(cudaMalloc(reinterpret_cast<void**>(&device_input.value), pixels));
    CUDA_CHECK(cudaMalloc(reinterpret_cast<void**>(&device_output.value), pixels));
    CUDA_CHECK(cudaMemcpy(device_input.value, input.data(), pixels, cudaMemcpyHostToDevice));
    for (unsigned iteration = 0; iteration < iterations; ++iteration) {
        edges<<<static_cast<unsigned>((pixels + 255) / 256), 256>>>(
            device_input.value, device_output.value, width, height, frames);
        CUDA_CHECK(cudaGetLastError());
        CUDA_CHECK(cudaDeviceSynchronize());
        if (iteration == 0) {
            FILE* started = std::fopen("started.json", "wx");
            if (!started) return failure("native_marker_failed");
            int count = std::fprintf(started,
                "{\"schema\":\"cohesix-registered-cuda-started/v1\","
                "\"device_uuid\":\"%s\",\"workload_id\":\"batch-edges\","
                "\"completed_iterations\":1}\n", uuid);
            bool synced = count > 0 && std::fflush(started) == 0
                && ::fsync(::fileno(started)) == 0;
            int closed = std::fclose(started);
            if (!synced || closed != 0) return failure("native_marker_failed");
        }
    }
    CUDA_CHECK(cudaMemcpy(output.data(), device_output.value, pixels, cudaMemcpyDeviceToHost));
    FILE* target = std::fopen("output.bin", "wbx");
    if (!target) return failure("output_exists_or_unavailable");
    size_t written = std::fwrite(output.data(), 1, pixels, target);
    int target_close = std::fclose(target);
    if (written != pixels || target_close != 0) return failure("output_write_failed");
    std::printf("{\"schema\":\"cohesix-registered-cuda-result/v1\","
        "\"workload_id\":\"batch-edges\",\"device_uuid\":\"%s\","
        "\"output_bytes\":%zu,\"allocation_bytes\":%zu,"
        "\"parameters\":{\"width\":%u,\"height\":%u,\"frames\":%u,\"iterations\":%u},"
        "\"free_bytes_before\":%zu,\"runtime_version\":%d,\"driver_version\":%d}\n",
        uuid, pixels, pixels * 2, width, height, frames, iterations, free_bytes, runtime, driver);
    return 0;
}
