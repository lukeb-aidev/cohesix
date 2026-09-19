// Author: Lukas Bower
// Purpose: Read bounded NVML MIG parent/instance/profile/placement identities without changing device configuration.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#pragma once

#include <nvml.h>
#include <dlfcn.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace cohesix_mig {
constexpr unsigned kMaximumInstances = 64;

inline int unavailable(const char* state, const char* reason, nvmlReturn_t code = NVML_SUCCESS) {
    std::printf("{\"schema\":\"cohesix-nvml-mig-topology/v1\",\"source\":\"nvml\",\"availability\":\"%s\",\"reason\":\"%s\",\"native_error\":%u,\"instances\":[]}\n",
        state, reason, static_cast<unsigned>(code));
    return 0;
}

inline bool uuid(char* value, size_t size, const char* prefix) {
    const size_t start = std::strlen(prefix);
    if (strnlen(value, size) != start + 36 || std::strncmp(value, prefix, start) != 0) return false;
    for (size_t index = 0; index < 36; ++index) {
        char& byte = value[start + index];
        if (index == 8 || index == 13 || index == 18 || index == 23) {
            if (byte != '-') return false;
        } else {
            if (byte >= 'A' && byte <= 'F') byte = static_cast<char>(byte + ('a' - 'A'));
            if (!((byte >= '0' && byte <= '9') || (byte >= 'a' && byte <= 'f'))) return false;
        }
    }
    return true;
}

struct Session {
    void* library = dlopen("libnvidia-ml.so.1", RTLD_NOW | RTLD_LOCAL);
    decltype(&nvmlShutdown) shutdown = nullptr;
    bool initialized = false;
    ~Session() {
        if (initialized && shutdown) shutdown();
        if (library) dlclose(library);
    }
    template<class Function> Function symbol(const char* name) {
        // The installed nvml.h owns each exact function-pointer ABI. POSIX
        // dlsym supplies the corresponding symbol; this Session owns its DSO
        // for every call and shuts NVML down before unloading it.
        return reinterpret_cast<Function>(dlsym(library, name));
    }
};

inline int inventory(unsigned ordinal) {
    Session session;
    if (!session.library) return unavailable("not_enabled", "nvml_library_unavailable");
#define COH_NVML_SYMBOL(name) const auto name##_call = session.symbol<decltype(&name)>(#name); \
    if (!name##_call) return unavailable("not_supported", "nvml_api_unavailable")
    COH_NVML_SYMBOL(nvmlInit_v2);
    COH_NVML_SYMBOL(nvmlShutdown);
    COH_NVML_SYMBOL(nvmlDeviceGetHandleByIndex_v2);
    COH_NVML_SYMBOL(nvmlDeviceGetUUID);
    COH_NVML_SYMBOL(nvmlDeviceGetMigMode);
    COH_NVML_SYMBOL(nvmlDeviceGetMaxMigDeviceCount);
    COH_NVML_SYMBOL(nvmlDeviceGetMigDeviceHandleByIndex);
    COH_NVML_SYMBOL(nvmlDeviceGetGpuInstanceId);
    COH_NVML_SYMBOL(nvmlDeviceGetComputeInstanceId);
    COH_NVML_SYMBOL(nvmlDeviceGetGpuInstanceById);
    COH_NVML_SYMBOL(nvmlGpuInstanceGetInfo);
    COH_NVML_SYMBOL(nvmlGpuInstanceGetComputeInstanceById);
    COH_NVML_SYMBOL(nvmlComputeInstanceGetInfo_v2);
    COH_NVML_SYMBOL(nvmlDeviceGetMemoryInfo);
#undef COH_NVML_SYMBOL
    session.shutdown = nvmlShutdown_call;
    nvmlReturn_t error = nvmlInit_v2_call();
    if (error != NVML_SUCCESS) return unavailable("unavailable", "nvml_initialization", error);
    session.initialized = true;
#define COH_NVML_READ(call) do { error = (call); if (error != NVML_SUCCESS) \
    return unavailable(error == NVML_ERROR_NOT_SUPPORTED ? "not_supported" : "unavailable", "nvml_observation", error); } while (false)
    nvmlDevice_t parent = nullptr;
    COH_NVML_READ(nvmlDeviceGetHandleByIndex_v2_call(ordinal, &parent));
    unsigned current = 0, pending = 0;
    COH_NVML_READ(nvmlDeviceGetMigMode_call(parent, &current, &pending));
    if (current != pending) return unavailable("unavailable", "mig_mode_change_pending");
    if (current == NVML_DEVICE_MIG_DISABLE) return unavailable("not_enabled", "mig_disabled");
    if (current != NVML_DEVICE_MIG_ENABLE) return unavailable("unavailable", "mig_mode_invalid");
    char parent_uuid[NVML_DEVICE_UUID_BUFFER_SIZE]{};
    COH_NVML_READ(nvmlDeviceGetUUID_call(parent, parent_uuid, sizeof(parent_uuid)));
    if (!uuid(parent_uuid, sizeof(parent_uuid), "GPU-")) return unavailable("unavailable", "parent_uuid_format");
    unsigned maximum = 0;
    COH_NVML_READ(nvmlDeviceGetMaxMigDeviceCount_call(parent, &maximum));
    if (maximum == 0 || maximum > kMaximumInstances) return unavailable("unavailable", "instance_count_bound");
    std::vector<std::string> rows;
    for (unsigned index = 0; index < maximum; ++index) {
        nvmlDevice_t device = nullptr;
        error = nvmlDeviceGetMigDeviceHandleByIndex_call(parent, index, &device);
        if (error == NVML_ERROR_NOT_FOUND) continue; // Sparse native instance slots are valid.
        if (error != NVML_SUCCESS) return unavailable("unavailable", "mig_instance_observation", error);
        char device_uuid[NVML_DEVICE_UUID_BUFFER_SIZE]{};
        COH_NVML_READ(nvmlDeviceGetUUID_call(device, device_uuid, sizeof(device_uuid)));
        if (!uuid(device_uuid, sizeof(device_uuid), "MIG-")) return unavailable("not_supported", "legacy_mig_uuid_format");
        unsigned gpu_id = 0, compute_id = 0;
        COH_NVML_READ(nvmlDeviceGetGpuInstanceId_call(device, &gpu_id));
        COH_NVML_READ(nvmlDeviceGetComputeInstanceId_call(device, &compute_id));
        nvmlGpuInstance_t gpu = nullptr;
        COH_NVML_READ(nvmlDeviceGetGpuInstanceById_call(parent, gpu_id, &gpu));
        nvmlGpuInstanceInfo_t gpu_info{};
        COH_NVML_READ(nvmlGpuInstanceGetInfo_call(gpu, &gpu_info));
        nvmlComputeInstance_t compute = nullptr;
        COH_NVML_READ(nvmlGpuInstanceGetComputeInstanceById_call(gpu, compute_id, &compute));
        nvmlComputeInstanceInfo_t compute_info{};
        COH_NVML_READ(nvmlComputeInstanceGetInfo_v2_call(compute, &compute_info));
        nvmlMemory_t memory{};
        COH_NVML_READ(nvmlDeviceGetMemoryInfo_call(device, &memory));
        if (gpu_info.device != parent || compute_info.device != parent || compute_info.gpuInstance != gpu
            || gpu_info.id != gpu_id || compute_info.id != compute_id || memory.total == 0
            || gpu_info.placement.size == 0 || gpu_info.placement.size > 64
            || gpu_info.placement.start > 63 || compute_info.placement.size == 0
            || compute_info.placement.size > gpu_info.placement.size
            || compute_info.placement.start > 63) return unavailable("unavailable", "instance_identity_or_bounds");
        char row[1024]{};
        int count = std::snprintf(row, sizeof(row),
            "{\"uuid\":\"%s\",\"gpu_instance_id\":%u,\"compute_instance_id\":%u,\"gpu_profile_id\":%u,\"compute_profile_id\":%u,\"memory_bytes\":%llu,\"gpu_placement\":{\"start\":%u,\"size\":%u},\"compute_placement\":{\"start\":%u,\"size\":%u}}",
            device_uuid, gpu_id, compute_id, gpu_info.profileId, compute_info.profileId,
            memory.total, gpu_info.placement.start, gpu_info.placement.size,
            compute_info.placement.start, compute_info.placement.size);
        if (count <= 0 || static_cast<size_t>(count) >= sizeof(row)) return unavailable("unavailable", "instance_bytes_bound");
        rows.emplace_back(row);
    }
    unsigned final_current = 0, final_pending = 0;
    COH_NVML_READ(nvmlDeviceGetMigMode_call(parent, &final_current, &final_pending));
    if (current != final_current || pending != final_pending) return unavailable("unavailable", "mig_mode_changed");
#undef COH_NVML_READ
    std::printf("{\"schema\":\"cohesix-nvml-mig-topology/v1\",\"source\":\"nvml\",\"availability\":\"observed\",\"parent_ordinal\":%u,\"parent_uuid\":\"%s\",\"mode\":\"enabled\",\"instances\":[", ordinal, parent_uuid);
    for (size_t index = 0; index < rows.size(); ++index) std::printf("%s%s", index ? "," : "", rows[index].c_str());
    std::printf("]}\n");
    return 0;
}
} // namespace cohesix_mig
