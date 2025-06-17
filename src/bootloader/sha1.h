// CLASSIFICATION: COMMUNITY
// Filename: sha1.h v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-08
// SPDX-License-Identifier: MIT
#include <stdint.h>
#include <stddef.h>

typedef struct {
    uint32_t state[5];
    uint64_t bitlen;
    uint8_t buffer[64];
    size_t  buflen;
} sha1_ctx;

void sha1_init(sha1_ctx *ctx);
void sha1_update(sha1_ctx *ctx, const uint8_t *data, size_t len);
void sha1_final(sha1_ctx *ctx, uint8_t digest[20]);
void sha1_to_hex(const uint8_t digest[20], char out[41]);

