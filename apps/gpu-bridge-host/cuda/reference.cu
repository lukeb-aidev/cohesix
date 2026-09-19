// Author: Lukas Bower
// Purpose: Execute only bounded CUDA reference kernels in a disposable gpu-bridge-host child with exact device identity and shared-memory admission.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

#include <cuda_runtime.h>
#include <cuda.h>
#include "mig_inventory.hpp"
#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <vector>
#include <csignal>
#include <sys/prctl.h>
#include <unistd.h>

#ifndef COH_REFERENCE_MIG
#define COH_REFERENCE_MIG 0
#endif

namespace {
constexpr size_t kHeadroom = size_t{2} << 30;
constexpr size_t kMaximumAllocation = size_t{64} << 20;

bool number(const char* value, unsigned maximum, unsigned* output) {
    if (!value || !*value) return false;
    for (const char* p = value; *p; ++p) if (*p < '0' || *p > '9') return false;
    errno = 0;
    char* end = nullptr;
    unsigned long parsed = std::strtoul(value, &end, 10);
    if (errno || *end || parsed > maximum) return false;
    *output = static_cast<unsigned>(parsed);
    return true;
}

int failure(const char* code, cudaError_t error = cudaSuccess) {
    std::fprintf(stderr, "{\"code\":\"%s\",\"cuda_error\":%d}\n", code, static_cast<int>(error));
    return error == cudaErrorMemoryAllocation ? 5 : 2;
}

struct DeviceBuffer {
    float* value = nullptr;
    ~DeviceBuffer() { if (value) cudaFree(value); }
};

__global__ void vector_add(const float* left, const float* right, float* output, unsigned count) {
    unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < count) output[i] = left[i] + right[i];
}

__global__ void matrix_multiply(const float* left, const float* right, float* output, unsigned n) {
    unsigned row = blockIdx.y * blockDim.y + threadIdx.y;
    unsigned column = blockIdx.x * blockDim.x + threadIdx.x;
    if (row < n && column < n) {
        float sum = 0.0f;
        for (unsigned k = 0; k < n; ++k) sum += left[row * n + k] * right[k * n + column];
        output[row * n + column] = sum;
    }
}
}

#define CUDA_CHECK(operation) do { cudaError_t error = (operation); \
    if (error != cudaSuccess) return failure("cuda_failure", error); } while (false)

int main(int argc, char** argv) {
    unsigned owner = 0;
    if (!number(std::getenv("COH_REFERENCE_OWNER_PID"), 2147483647, &owner)
        || owner == 0 || static_cast<unsigned>(getppid()) != owner
        || prctl(PR_SET_PDEATHSIG, SIGKILL) != 0
        || static_cast<unsigned>(getppid()) != owner) return failure("owner_binding");
    if (argc != 3 && argc != 6) return failure("invalid_request");
    unsigned ordinal = 0;
    if (!number(argv[2], 31, &ordinal)) return failure("invalid_device");
    if (std::strcmp(argv[1], "mig-inventory") == 0) {
        if (argc != 3) return failure("invalid_request");
        return cohesix_mig::inventory(ordinal);
    }
    bool inventory = std::strcmp(argv[1], "inventory") == 0;
    bool vadd = std::strcmp(argv[1], "vadd") == 0;
    bool matmul = std::strcmp(argv[1], "matmul") == 0;
    if ((inventory && argc != 3) || (!inventory && (!vadd && !matmul))) return failure("invalid_entrypoint");
    if (!inventory && argc != 6) return failure("invalid_request");
    char selected_mig[NVML_DEVICE_UUID_BUFFER_SIZE]{};
    if (COH_REFERENCE_MIG) {
        const char* selected = std::getenv("CUDA_VISIBLE_DEVICES");
        if (!selected || std::strlen(selected) >= sizeof(selected_mig) || ordinal != 0)
            return failure("profile_mismatch");
        std::memcpy(selected_mig, selected, std::strlen(selected) + 1);
        if (!cohesix_mig::uuid(selected_mig, sizeof(selected_mig), "MIG-")) return failure("profile_mismatch");
    }
    CUDA_CHECK(cudaSetDevice(static_cast<int>(ordinal)));
    cudaDeviceProp properties{};
    CUDA_CHECK(cudaGetDeviceProperties(&properties, static_cast<int>(ordinal)));
    char uuid[33]{};
    for (unsigned i = 0; i < 16; ++i) std::snprintf(uuid + i * 2, 3, "%02x", static_cast<unsigned char>(properties.uuid.bytes[i]));
    if (COH_REFERENCE_MIG) {
        CUdevice device = 0;
        CUuuid identity{};
        if (cuInit(0) != CUDA_SUCCESS || cuDeviceGet(&device, static_cast<int>(ordinal)) != CUDA_SUCCESS
            || cuDeviceGetUuid_v2(&identity, device) != CUDA_SUCCESS) return failure("device_identity_unavailable");
        for (unsigned i = 0; i < 16; ++i) std::snprintf(uuid + i * 2, 3, "%02x", static_cast<unsigned char>(identity.bytes[i]));
        char expected[33]{};
        unsigned output = 0;
        for (unsigned i = 4; selected_mig[i]; ++i) if (selected_mig[i] != '-') expected[output++] = selected_mig[i];
        if (std::strcmp(uuid, expected) != 0) return failure("wrong_device");
    }
    if (std::strcmp(uuid, "00000000000000000000000000000000") == 0) return failure("device_identity_unavailable");
    size_t free_bytes = 0, total_bytes = 0;
    CUDA_CHECK(cudaMemGetInfo(&free_bytes, &total_bytes));
    int runtime = 0, driver = 0;
    CUDA_CHECK(cudaRuntimeGetVersion(&runtime));
    CUDA_CHECK(cudaDriverGetVersion(&driver));
    if (runtime != 13020 || (COH_REFERENCE_MIG
        ? (properties.integrated || properties.major < 8 || driver < 13020)
        : (properties.major != 8 || properties.minor != 7 || !properties.integrated)))
        return failure("profile_mismatch");
    if (inventory) {
        std::printf("{\"schema\":\"cohesix-cuda-native-observation/v1\",\"device_ordinal\":%u,\"device_uuid\":\"%s\",\"free_bytes\":%zu,\"total_bytes\":%zu,\"sm_count\":%d,\"compute_major\":%d,\"compute_minor\":%d,\"integrated\":%s,\"runtime_version\":%d,\"driver_version\":%d}\n",
            ordinal, uuid, free_bytes, total_bytes, properties.multiProcessorCount, properties.major, properties.minor,
            properties.integrated ? "true" : "false", runtime, driver);
        return 0;
    }
    if (std::strcmp(uuid, argv[3]) != 0) return failure("wrong_device");
    unsigned dimension = 0, iterations = 0;
    if (!number(argv[4], vadd ? 65536 : 128, &dimension) || dimension == 0
        || !number(argv[5], 10000, &iterations) || iterations == 0) return failure("invalid_bound");
    size_t count = vadd ? dimension : size_t{dimension} * dimension;
    size_t bytes = count * sizeof(float);
    size_t allocation = bytes * 3;
    if (allocation > kMaximumAllocation || free_bytes < kHeadroom || allocation > free_bytes - kHeadroom)
        return failure("memory_admission");
    std::vector<float> left(count), right(count), output(count);
    for (size_t i = 0; i < count; ++i) {
        left[i] = vadd ? static_cast<float>(i % 1024) : static_cast<float>(i / dimension + 1);
        right[i] = vadd ? static_cast<float>(2 * (i % 1024)) : static_cast<float>(i % dimension + 1);
    }
    DeviceBuffer a, b, c;
    CUDA_CHECK(cudaMalloc(reinterpret_cast<void**>(&a.value), bytes));
    CUDA_CHECK(cudaMalloc(reinterpret_cast<void**>(&b.value), bytes));
    CUDA_CHECK(cudaMalloc(reinterpret_cast<void**>(&c.value), bytes));
    CUDA_CHECK(cudaMemcpy(a.value, left.data(), bytes, cudaMemcpyHostToDevice));
    CUDA_CHECK(cudaMemcpy(b.value, right.data(), bytes, cudaMemcpyHostToDevice));
    for (unsigned iteration = 0; iteration < iterations; ++iteration) {
        if (vadd) vector_add<<<(dimension + 255) / 256, 256>>>(a.value, b.value, c.value, dimension);
        else matrix_multiply<<<dim3((dimension + 15) / 16, (dimension + 15) / 16), dim3(16, 16)>>>(a.value, b.value, c.value, dimension);
        CUDA_CHECK(cudaGetLastError());
        CUDA_CHECK(cudaDeviceSynchronize());
        if (iteration == 0) {
            // A bounded durable marker proves one completed kernel before live cancellation tests.
            FILE* started = std::fopen("started.json", "wx");
            if (!started) return failure("native_marker_failed");
            const int count = std::fprintf(started, "{\"schema\":\"cohesix-cuda-started/v1\",\"device_uuid\":\"%s\",\"entrypoint\":\"%s\",\"completed_iterations\":1}\n", uuid, argv[1]);
            const bool synced = count > 0 && std::fflush(started) == 0 && ::fsync(::fileno(started)) == 0;
            const int closed = std::fclose(started);
            if (!synced || closed != 0) return failure("native_marker_failed");
        }
    }
    CUDA_CHECK(cudaMemcpy(output.data(), c.value, bytes, cudaMemcpyDeviceToHost));
    // The owner supplies a fresh private working directory; the helper accepts no path argument.
    FILE* file = std::fopen("output.bin", "wbx");
    if (!file) return failure("output_exists_or_unavailable");
    size_t written = std::fwrite(output.data(), 1, bytes, file);
    int closed = std::fclose(file);
    if (written != bytes || closed != 0) return failure("output_write_failed");
    std::printf("{\"schema\":\"cohesix-cuda-native-result/v1\",\"entrypoint\":\"%s\",\"device_uuid\":\"%s\",\"device_ordinal\":%u,\"dimension\":%u,\"iterations\":%u,\"output_bytes\":%zu,\"allocation_bytes\":%zu,\"free_bytes_before\":%zu,\"runtime_version\":%d,\"driver_version\":%d}\n",
        argv[1], uuid, ordinal, dimension, iterations, bytes, allocation, free_bytes, runtime, driver);
    return 0;
}
