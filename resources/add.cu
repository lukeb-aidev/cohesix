// CLASSIFICATION: COMMUNITY
// Filename: add.cu v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20

extern "C" __global__ void sum(const float* a, const float* b, float* out, int n) {
    int idx = threadIdx.x + blockIdx.x * blockDim.x;
    if (idx < n) {
        out[idx] = a[idx] + b[idx];
    }
}
