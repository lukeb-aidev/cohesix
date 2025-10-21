/* generated from /Users/lukasbower/seL4/kernel/libsel4/mode_include/64/sel4/shared_types.bf */

#pragma once

#include <config.h>
#include <assert.h>
#include <stdint.h>
#include <util.h>
struct seL4_CNode_CapData {
    uint64_t words[1];
};
typedef struct seL4_CNode_CapData seL4_CNode_CapData_t;

static inline uint64_t CONST
seL4_CNode_CapData_get_guard(seL4_CNode_CapData_t seL4_CNode_CapData) {
    uint64_t ret;
    ret = (seL4_CNode_CapData.words[0] & 0xffffffffffffffc0ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_CNode_CapData_get_guardSize(seL4_CNode_CapData_t seL4_CNode_CapData) {
    uint64_t ret;
    ret = (seL4_CNode_CapData.words[0] & 0x3full) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

struct seL4_CapRights {
    uint64_t words[1];
};
typedef struct seL4_CapRights seL4_CapRights_t;

static inline uint64_t CONST
seL4_CapRights_get_capAllowGrantReply(seL4_CapRights_t seL4_CapRights) {
    uint64_t ret;
    ret = (seL4_CapRights.words[0] & 0x8ull) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_CapRights_get_capAllowGrant(seL4_CapRights_t seL4_CapRights) {
    uint64_t ret;
    ret = (seL4_CapRights.words[0] & 0x4ull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_CapRights_get_capAllowRead(seL4_CapRights_t seL4_CapRights) {
    uint64_t ret;
    ret = (seL4_CapRights.words[0] & 0x2ull) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_CapRights_get_capAllowWrite(seL4_CapRights_t seL4_CapRights) {
    uint64_t ret;
    ret = (seL4_CapRights.words[0] & 0x1ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

struct seL4_MessageInfo {
    uint64_t words[1];
};
typedef struct seL4_MessageInfo seL4_MessageInfo_t;

static inline seL4_MessageInfo_t CONST
seL4_MessageInfo_new(uint64_t label, uint64_t capsUnwrapped, uint64_t extraCaps, uint64_t length) {
    seL4_MessageInfo_t seL4_MessageInfo;

    /* fail if user has passed bits that we will override */  
    assert((label & ~0xfffffffffffffull) == ((0 && (label & (1ull << 63))) ? 0x0 : 0));  
    assert((capsUnwrapped & ~0x7ull) == ((0 && (capsUnwrapped & (1ull << 63))) ? 0x0 : 0));  
    assert((extraCaps & ~0x3ull) == ((0 && (extraCaps & (1ull << 63))) ? 0x0 : 0));  
    assert((length & ~0x7full) == ((0 && (length & (1ull << 63))) ? 0x0 : 0));

    seL4_MessageInfo.words[0] = 0
        | (label & 0xfffffffffffffull) << 12
        | (capsUnwrapped & 0x7ull) << 9
        | (extraCaps & 0x3ull) << 7
        | (length & 0x7full) << 0;

    return seL4_MessageInfo;
}

static inline uint64_t CONST
seL4_MessageInfo_get_label(seL4_MessageInfo_t seL4_MessageInfo) {
    uint64_t ret;
    ret = (seL4_MessageInfo.words[0] & 0xfffffffffffff000ull) >> 12;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_MessageInfo_get_capsUnwrapped(seL4_MessageInfo_t seL4_MessageInfo) {
    uint64_t ret;
    ret = (seL4_MessageInfo.words[0] & 0xe00ull) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_MessageInfo_t CONST
seL4_MessageInfo_set_capsUnwrapped(seL4_MessageInfo_t seL4_MessageInfo, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xe00ull >> 9 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (63)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0xe00ull;
    seL4_MessageInfo.words[0] |= (v64 << 9) & 0xe00ull;
    return seL4_MessageInfo;
}

static inline uint64_t CONST
seL4_MessageInfo_get_extraCaps(seL4_MessageInfo_t seL4_MessageInfo) {
    uint64_t ret;
    ret = (seL4_MessageInfo.words[0] & 0x180ull) >> 7;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_MessageInfo_t CONST
seL4_MessageInfo_set_extraCaps(seL4_MessageInfo_t seL4_MessageInfo, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x180ull >> 7 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (63)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0x180ull;
    seL4_MessageInfo.words[0] |= (v64 << 7) & 0x180ull;
    return seL4_MessageInfo;
}

static inline uint64_t CONST
seL4_MessageInfo_get_length(seL4_MessageInfo_t seL4_MessageInfo) {
    uint64_t ret;
    ret = (seL4_MessageInfo.words[0] & 0x7full) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (63)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_MessageInfo_t CONST
seL4_MessageInfo_set_length(seL4_MessageInfo_t seL4_MessageInfo, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x7full >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (63)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0x7full;
    seL4_MessageInfo.words[0] |= (v64 << 0) & 0x7full;
    return seL4_MessageInfo;
}

