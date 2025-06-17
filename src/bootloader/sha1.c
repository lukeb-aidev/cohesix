// CLASSIFICATION: COMMUNITY
// Filename: sha1.c v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-08
// SPDX-License-Identifier: MIT
#include <stdint.h>
#include <stddef.h>
#include <string.h>

typedef struct {
    uint32_t state[5];
    uint64_t bitlen;
    uint8_t buffer[64];
    size_t  buflen;
} sha1_ctx;

static uint32_t rotl(uint32_t x, uint32_t n) { return (x << n) | (x >> (32 - n)); }

static void sha1_transform(uint32_t state[5], const uint8_t block[64])
{
    uint32_t w[80];
    for (int i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i * 4] << 24) |
                ((uint32_t)block[i * 4 + 1] << 16) |
                ((uint32_t)block[i * 4 + 2] << 8) |
                ((uint32_t)block[i * 4 + 3]);
    }
    for (int i = 16; i < 80; i++)
        w[i] = rotl(w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16], 1);

    uint32_t a = state[0];
    uint32_t b = state[1];
    uint32_t c = state[2];
    uint32_t d = state[3];
    uint32_t e = state[4];

    for (int i = 0; i < 80; i++) {
        uint32_t f, k;
        if (i < 20) {
            f = (b & c) | (~b & d);
            k = 0x5A827999;
        } else if (i < 40) {
            f = b ^ c ^ d;
            k = 0x6ED9EBA1;
        } else if (i < 60) {
            f = (b & c) | (b & d) | (c & d);
            k = 0x8F1BBCDC;
        } else {
            f = b ^ c ^ d;
            k = 0xCA62C1D6;
        }
        uint32_t temp = rotl(a,5) + f + e + k + w[i];
        e = d;
        d = c;
        c = rotl(b,30);
        b = a;
        a = temp;
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
}

void sha1_init(sha1_ctx *ctx)
{
    ctx->state[0] = 0x67452301;
    ctx->state[1] = 0xEFCDAB89;
    ctx->state[2] = 0x98BADCFE;
    ctx->state[3] = 0x10325476;
    ctx->state[4] = 0xC3D2E1F0;
    ctx->bitlen = 0;
    ctx->buflen = 0;
}

void sha1_update(sha1_ctx *ctx, const uint8_t *data, size_t len)
{
    ctx->bitlen += (uint64_t)len * 8;
    while (len > 0) {
        size_t n = 64 - ctx->buflen;
        if (n > len) n = len;
        memcpy(ctx->buffer + ctx->buflen, data, n);
        ctx->buflen += n;
        data += n;
        len -= n;
        if (ctx->buflen == 64) {
            sha1_transform(ctx->state, ctx->buffer);
            ctx->buflen = 0;
        }
    }
}

void sha1_final(sha1_ctx *ctx, uint8_t digest[20])
{
    ctx->buffer[ctx->buflen++] = 0x80;
    if (ctx->buflen > 56) {
        while (ctx->buflen < 64)
            ctx->buffer[ctx->buflen++] = 0;
        sha1_transform(ctx->state, ctx->buffer);
        ctx->buflen = 0;
    }
    while (ctx->buflen < 56)
        ctx->buffer[ctx->buflen++] = 0;

    uint64_t be = (ctx->bitlen);
    for (int i = 0; i < 8; i++)
        ctx->buffer[56 + i] = (uint8_t)(be >> (56 - 8 * i));
    sha1_transform(ctx->state, ctx->buffer);

    for (int i = 0; i < 5; i++) {
        digest[i*4]     = (uint8_t)(ctx->state[i] >> 24);
        digest[i*4 + 1] = (uint8_t)(ctx->state[i] >> 16);
        digest[i*4 + 2] = (uint8_t)(ctx->state[i] >> 8);
        digest[i*4 + 3] = (uint8_t)(ctx->state[i]);
    }
}

void sha1_to_hex(const uint8_t digest[20], char out[41])
{
    static const char *hex = "0123456789abcdef";
    for (int i = 0; i < 20; i++) {
        out[i*2] = hex[digest[i] >> 4];
        out[i*2+1] = hex[digest[i] & 0x0F];
    }
    out[40] = '\0';
}

