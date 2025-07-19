/* generated from /home/ubuntu/sel4_build/kernel/libsel4/mode_include/32/sel4/shared_types.bf */

#pragma once

#include <sel4/config.h>
#include <sel4/simple_types.h>
#include <sel4/debug_assert.h>
struct seL4_CNode_CapData {
    seL4_Uint32 words[1];
};
typedef struct seL4_CNode_CapData seL4_CNode_CapData_t;

LIBSEL4_INLINE_FUNC seL4_CNode_CapData_t CONST
seL4_CNode_CapData_new(seL4_Uint32 guard, seL4_Uint32 guardSize) {
    seL4_CNode_CapData_t seL4_CNode_CapData;

    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((guard & ~0x3ffffu) == ((0 && (guard & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((guardSize & ~0x1fu) == ((0 && (guardSize & (1u << 31))) ? 0x0 : 0));

    seL4_CNode_CapData.words[0] = 0
        | (guard & 0x3ffffu) << 8
        | (guardSize & 0x1fu) << 3;

    return seL4_CNode_CapData;
}

LIBSEL4_INLINE_FUNC void
seL4_CNode_CapData_ptr_new(seL4_CNode_CapData_t *seL4_CNode_CapData_ptr, seL4_Uint32 guard, seL4_Uint32 guardSize) {
    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((guard & ~0x3ffffu) == ((0 && (guard & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((guardSize & ~0x1fu) == ((0 && (guardSize & (1u << 31))) ? 0x0 : 0));

    seL4_CNode_CapData_ptr->words[0] = 0
        | (guard & 0x3ffffu) << 8
        | (guardSize & 0x1fu) << 3;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CNode_CapData_get_guard(seL4_CNode_CapData_t seL4_CNode_CapData) {
    seL4_Uint32 ret;
    ret = (seL4_CNode_CapData.words[0] & 0x3ffff00u) >> 8;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CNode_CapData_t CONST
seL4_CNode_CapData_set_guard(seL4_CNode_CapData_t seL4_CNode_CapData, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x3ffff00u >> 8 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CNode_CapData.words[0] &= ~0x3ffff00u;
    seL4_CNode_CapData.words[0] |= (v32 << 8) & 0x3ffff00u;
    return seL4_CNode_CapData;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CNode_CapData_ptr_get_guard(seL4_CNode_CapData_t *seL4_CNode_CapData_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CNode_CapData_ptr->words[0] & 0x3ffff00u) >> 8;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CNode_CapData_ptr_set_guard(seL4_CNode_CapData_t *seL4_CNode_CapData_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x3ffff00u >> 8) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CNode_CapData_ptr->words[0] &= ~0x3ffff00u;
    seL4_CNode_CapData_ptr->words[0] |= (v32 << 8) & 0x3ffff00;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CNode_CapData_get_guardSize(seL4_CNode_CapData_t seL4_CNode_CapData) {
    seL4_Uint32 ret;
    ret = (seL4_CNode_CapData.words[0] & 0xf8u) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CNode_CapData_t CONST
seL4_CNode_CapData_set_guardSize(seL4_CNode_CapData_t seL4_CNode_CapData, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xf8u >> 3 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CNode_CapData.words[0] &= ~0xf8u;
    seL4_CNode_CapData.words[0] |= (v32 << 3) & 0xf8u;
    return seL4_CNode_CapData;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CNode_CapData_ptr_get_guardSize(seL4_CNode_CapData_t *seL4_CNode_CapData_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CNode_CapData_ptr->words[0] & 0xf8u) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CNode_CapData_ptr_set_guardSize(seL4_CNode_CapData_t *seL4_CNode_CapData_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xf8u >> 3) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CNode_CapData_ptr->words[0] &= ~0xf8u;
    seL4_CNode_CapData_ptr->words[0] |= (v32 << 3) & 0xf8;
}

struct seL4_CapRights {
    seL4_Uint32 words[1];
};
typedef struct seL4_CapRights seL4_CapRights_t;

LIBSEL4_INLINE_FUNC seL4_CapRights_t CONST
seL4_CapRights_new(seL4_Uint32 capAllowGrantReply, seL4_Uint32 capAllowGrant, seL4_Uint32 capAllowRead, seL4_Uint32 capAllowWrite) {
    seL4_CapRights_t seL4_CapRights;

    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((capAllowGrantReply & ~0x1u) == ((0 && (capAllowGrantReply & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowGrant & ~0x1u) == ((0 && (capAllowGrant & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowRead & ~0x1u) == ((0 && (capAllowRead & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowWrite & ~0x1u) == ((0 && (capAllowWrite & (1u << 31))) ? 0x0 : 0));

    seL4_CapRights.words[0] = 0
        | (capAllowGrantReply & 0x1u) << 3
        | (capAllowGrant & 0x1u) << 2
        | (capAllowRead & 0x1u) << 1
        | (capAllowWrite & 0x1u) << 0;

    return seL4_CapRights;
}

LIBSEL4_INLINE_FUNC void
seL4_CapRights_ptr_new(seL4_CapRights_t *seL4_CapRights_ptr, seL4_Uint32 capAllowGrantReply, seL4_Uint32 capAllowGrant, seL4_Uint32 capAllowRead, seL4_Uint32 capAllowWrite) {
    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((capAllowGrantReply & ~0x1u) == ((0 && (capAllowGrantReply & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowGrant & ~0x1u) == ((0 && (capAllowGrant & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowRead & ~0x1u) == ((0 && (capAllowRead & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capAllowWrite & ~0x1u) == ((0 && (capAllowWrite & (1u << 31))) ? 0x0 : 0));

    seL4_CapRights_ptr->words[0] = 0
        | (capAllowGrantReply & 0x1u) << 3
        | (capAllowGrant & 0x1u) << 2
        | (capAllowRead & 0x1u) << 1
        | (capAllowWrite & 0x1u) << 0;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CapRights_get_capAllowGrantReply(seL4_CapRights_t seL4_CapRights) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights.words[0] & 0x8u) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CapRights_t CONST
seL4_CapRights_set_capAllowGrantReply(seL4_CapRights_t seL4_CapRights, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x8u >> 3 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights.words[0] &= ~0x8u;
    seL4_CapRights.words[0] |= (v32 << 3) & 0x8u;
    return seL4_CapRights;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CapRights_ptr_get_capAllowGrantReply(seL4_CapRights_t *seL4_CapRights_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights_ptr->words[0] & 0x8u) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CapRights_ptr_set_capAllowGrantReply(seL4_CapRights_t *seL4_CapRights_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x8u >> 3) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights_ptr->words[0] &= ~0x8u;
    seL4_CapRights_ptr->words[0] |= (v32 << 3) & 0x8;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CapRights_get_capAllowGrant(seL4_CapRights_t seL4_CapRights) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights.words[0] & 0x4u) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CapRights_t CONST
seL4_CapRights_set_capAllowGrant(seL4_CapRights_t seL4_CapRights, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x4u >> 2 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights.words[0] &= ~0x4u;
    seL4_CapRights.words[0] |= (v32 << 2) & 0x4u;
    return seL4_CapRights;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CapRights_ptr_get_capAllowGrant(seL4_CapRights_t *seL4_CapRights_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights_ptr->words[0] & 0x4u) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CapRights_ptr_set_capAllowGrant(seL4_CapRights_t *seL4_CapRights_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x4u >> 2) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights_ptr->words[0] &= ~0x4u;
    seL4_CapRights_ptr->words[0] |= (v32 << 2) & 0x4;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CapRights_get_capAllowRead(seL4_CapRights_t seL4_CapRights) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights.words[0] & 0x2u) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CapRights_t CONST
seL4_CapRights_set_capAllowRead(seL4_CapRights_t seL4_CapRights, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x2u >> 1 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights.words[0] &= ~0x2u;
    seL4_CapRights.words[0] |= (v32 << 1) & 0x2u;
    return seL4_CapRights;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CapRights_ptr_get_capAllowRead(seL4_CapRights_t *seL4_CapRights_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights_ptr->words[0] & 0x2u) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CapRights_ptr_set_capAllowRead(seL4_CapRights_t *seL4_CapRights_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x2u >> 1) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights_ptr->words[0] &= ~0x2u;
    seL4_CapRights_ptr->words[0] |= (v32 << 1) & 0x2;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_CapRights_get_capAllowWrite(seL4_CapRights_t seL4_CapRights) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights.words[0] & 0x1u) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_CapRights_t CONST
seL4_CapRights_set_capAllowWrite(seL4_CapRights_t seL4_CapRights, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x1u >> 0 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights.words[0] &= ~0x1u;
    seL4_CapRights.words[0] |= (v32 << 0) & 0x1u;
    return seL4_CapRights;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_CapRights_ptr_get_capAllowWrite(seL4_CapRights_t *seL4_CapRights_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_CapRights_ptr->words[0] & 0x1u) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_CapRights_ptr_set_capAllowWrite(seL4_CapRights_t *seL4_CapRights_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x1u >> 0) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_CapRights_ptr->words[0] &= ~0x1u;
    seL4_CapRights_ptr->words[0] |= (v32 << 0) & 0x1;
}

struct seL4_MessageInfo {
    seL4_Uint32 words[1];
};
typedef struct seL4_MessageInfo seL4_MessageInfo_t;

LIBSEL4_INLINE_FUNC seL4_MessageInfo_t CONST
seL4_MessageInfo_new(seL4_Uint32 label, seL4_Uint32 capsUnwrapped, seL4_Uint32 extraCaps, seL4_Uint32 length) {
    seL4_MessageInfo_t seL4_MessageInfo;

    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((label & ~0xfffffu) == ((0 && (label & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capsUnwrapped & ~0x7u) == ((0 && (capsUnwrapped & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((extraCaps & ~0x3u) == ((0 && (extraCaps & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((length & ~0x7fu) == ((0 && (length & (1u << 31))) ? 0x0 : 0));

    seL4_MessageInfo.words[0] = 0
        | (label & 0xfffffu) << 12
        | (capsUnwrapped & 0x7u) << 9
        | (extraCaps & 0x3u) << 7
        | (length & 0x7fu) << 0;

    return seL4_MessageInfo;
}

LIBSEL4_INLINE_FUNC void
seL4_MessageInfo_ptr_new(seL4_MessageInfo_t *seL4_MessageInfo_ptr, seL4_Uint32 label, seL4_Uint32 capsUnwrapped, seL4_Uint32 extraCaps, seL4_Uint32 length) {
    /* fail if user has passed bits that we will override */  
    seL4_DebugAssert((label & ~0xfffffu) == ((0 && (label & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((capsUnwrapped & ~0x7u) == ((0 && (capsUnwrapped & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((extraCaps & ~0x3u) == ((0 && (extraCaps & (1u << 31))) ? 0x0 : 0));  
    seL4_DebugAssert((length & ~0x7fu) == ((0 && (length & (1u << 31))) ? 0x0 : 0));

    seL4_MessageInfo_ptr->words[0] = 0
        | (label & 0xfffffu) << 12
        | (capsUnwrapped & 0x7u) << 9
        | (extraCaps & 0x3u) << 7
        | (length & 0x7fu) << 0;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_MessageInfo_get_label(seL4_MessageInfo_t seL4_MessageInfo) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo.words[0] & 0xfffff000u) >> 12;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_MessageInfo_t CONST
seL4_MessageInfo_set_label(seL4_MessageInfo_t seL4_MessageInfo, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xfffff000u >> 12 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0xfffff000u;
    seL4_MessageInfo.words[0] |= (v32 << 12) & 0xfffff000u;
    return seL4_MessageInfo;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_MessageInfo_ptr_get_label(seL4_MessageInfo_t *seL4_MessageInfo_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo_ptr->words[0] & 0xfffff000u) >> 12;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_MessageInfo_ptr_set_label(seL4_MessageInfo_t *seL4_MessageInfo_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xfffff000u >> 12) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo_ptr->words[0] &= ~0xfffff000u;
    seL4_MessageInfo_ptr->words[0] |= (v32 << 12) & 0xfffff000;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_MessageInfo_get_capsUnwrapped(seL4_MessageInfo_t seL4_MessageInfo) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo.words[0] & 0xe00u) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_MessageInfo_t CONST
seL4_MessageInfo_set_capsUnwrapped(seL4_MessageInfo_t seL4_MessageInfo, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xe00u >> 9 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0xe00u;
    seL4_MessageInfo.words[0] |= (v32 << 9) & 0xe00u;
    return seL4_MessageInfo;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_MessageInfo_ptr_get_capsUnwrapped(seL4_MessageInfo_t *seL4_MessageInfo_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo_ptr->words[0] & 0xe00u) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_MessageInfo_ptr_set_capsUnwrapped(seL4_MessageInfo_t *seL4_MessageInfo_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0xe00u >> 9) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo_ptr->words[0] &= ~0xe00u;
    seL4_MessageInfo_ptr->words[0] |= (v32 << 9) & 0xe00;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_MessageInfo_get_extraCaps(seL4_MessageInfo_t seL4_MessageInfo) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo.words[0] & 0x180u) >> 7;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_MessageInfo_t CONST
seL4_MessageInfo_set_extraCaps(seL4_MessageInfo_t seL4_MessageInfo, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x180u >> 7 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0x180u;
    seL4_MessageInfo.words[0] |= (v32 << 7) & 0x180u;
    return seL4_MessageInfo;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_MessageInfo_ptr_get_extraCaps(seL4_MessageInfo_t *seL4_MessageInfo_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo_ptr->words[0] & 0x180u) >> 7;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_MessageInfo_ptr_set_extraCaps(seL4_MessageInfo_t *seL4_MessageInfo_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x180u >> 7) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo_ptr->words[0] &= ~0x180u;
    seL4_MessageInfo_ptr->words[0] |= (v32 << 7) & 0x180;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 CONST
seL4_MessageInfo_get_length(seL4_MessageInfo_t seL4_MessageInfo) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo.words[0] & 0x7fu) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC seL4_MessageInfo_t CONST
seL4_MessageInfo_set_length(seL4_MessageInfo_t seL4_MessageInfo, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x7fu >> 0 ) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo.words[0] &= ~0x7fu;
    seL4_MessageInfo.words[0] |= (v32 << 0) & 0x7fu;
    return seL4_MessageInfo;
}

LIBSEL4_INLINE_FUNC seL4_Uint32 PURE
seL4_MessageInfo_ptr_get_length(seL4_MessageInfo_t *seL4_MessageInfo_ptr) {
    seL4_Uint32 ret;
    ret = (seL4_MessageInfo_ptr->words[0] & 0x7fu) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1u << (31)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

LIBSEL4_INLINE_FUNC void
seL4_MessageInfo_ptr_set_length(seL4_MessageInfo_t *seL4_MessageInfo_ptr, seL4_Uint32 v32) {
    /* fail if user has passed bits that we will override */
    seL4_DebugAssert((((~0x7fu >> 0) | 0x0) & v32) == ((0 && (v32 & (1u << (31)))) ? 0x0 : 0));
    seL4_MessageInfo_ptr->words[0] &= ~0x7fu;
    seL4_MessageInfo_ptr->words[0] |= (v32 << 0) & 0x7f;
}

