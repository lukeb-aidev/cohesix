/* generated from /Users/lukasbower/seL4/kernel/include/arch/arm/arch/object/structures.bf */

#pragma once

#include <config.h>
#include <assert.h>
#include <stdint.h>
#include <util.h>
struct endpoint {
    uint64_t words[2];
};
typedef struct endpoint endpoint_t;

static inline uint64_t PURE
endpoint_ptr_get_epQueue_head(endpoint_t *endpoint_ptr) {
    uint64_t ret;
    ret = (endpoint_ptr->words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
endpoint_ptr_set_epQueue_head(endpoint_t *endpoint_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    endpoint_ptr->words[1] &= ~0xffffffffffffffffull;
    endpoint_ptr->words[1] |= (v64 << 0) & 0xffffffffffffffff;
}

static inline uint64_t PURE
endpoint_ptr_get_epQueue_tail(endpoint_t *endpoint_ptr) {
    uint64_t ret;
    ret = (endpoint_ptr->words[0] & 0xfffffffffffcull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
endpoint_ptr_set_epQueue_tail(endpoint_t *endpoint_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xfffffffffffcull << 0) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    endpoint_ptr->words[0] &= ~0xfffffffffffcull;
    endpoint_ptr->words[0] |= (v64 >> 0) & 0xfffffffffffc;
}

static inline uint64_t PURE
endpoint_ptr_get_state(endpoint_t *endpoint_ptr) {
    uint64_t ret;
    ret = (endpoint_ptr->words[0] & 0x3ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
endpoint_ptr_set_state(endpoint_t *endpoint_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x3ull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    endpoint_ptr->words[0] &= ~0x3ull;
    endpoint_ptr->words[0] |= (v64 << 0) & 0x3;
}

struct mdb_node {
    uint64_t words[2];
};
typedef struct mdb_node mdb_node_t;

static inline mdb_node_t CONST
mdb_node_new(uint64_t mdbNext, uint64_t mdbRevocable, uint64_t mdbFirstBadged, uint64_t mdbPrev) {
    mdb_node_t mdb_node;

    /* fail if user has passed bits that we will override */  
    assert((mdbNext & ~0xfffffffffffcull) == ((1 && (mdbNext & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert((mdbRevocable & ~0x1ull) == ((1 && (mdbRevocable & (1ull << 47))) ? 0x0 : 0));  
    assert((mdbFirstBadged & ~0x1ull) == ((1 && (mdbFirstBadged & (1ull << 47))) ? 0x0 : 0));

    mdb_node.words[0] = 0
        | mdbPrev << 0;;
    mdb_node.words[1] = 0
        | (mdbNext & 0xfffffffffffcull) >> 0
        | (mdbRevocable & 0x1ull) << 1
        | (mdbFirstBadged & 0x1ull) << 0;

    return mdb_node;
}

static inline uint64_t CONST
mdb_node_get_mdbNext(mdb_node_t mdb_node) {
    uint64_t ret;
    ret = (mdb_node.words[1] & 0xfffffffffffcull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
mdb_node_ptr_set_mdbNext(mdb_node_t *mdb_node_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xfffffffffffcull << 0) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    mdb_node_ptr->words[1] &= ~0xfffffffffffcull;
    mdb_node_ptr->words[1] |= (v64 >> 0) & 0xfffffffffffc;
}

static inline uint64_t CONST
mdb_node_get_mdbRevocable(mdb_node_t mdb_node) {
    uint64_t ret;
    ret = (mdb_node.words[1] & 0x2ull) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline mdb_node_t CONST
mdb_node_set_mdbRevocable(mdb_node_t mdb_node, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x2ull >> 1 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node.words[1] &= ~0x2ull;
    mdb_node.words[1] |= (v64 << 1) & 0x2ull;
    return mdb_node;
}

static inline void
mdb_node_ptr_set_mdbRevocable(mdb_node_t *mdb_node_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x2ull >> 1) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node_ptr->words[1] &= ~0x2ull;
    mdb_node_ptr->words[1] |= (v64 << 1) & 0x2;
}

static inline uint64_t CONST
mdb_node_get_mdbFirstBadged(mdb_node_t mdb_node) {
    uint64_t ret;
    ret = (mdb_node.words[1] & 0x1ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline mdb_node_t CONST
mdb_node_set_mdbFirstBadged(mdb_node_t mdb_node, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x1ull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node.words[1] &= ~0x1ull;
    mdb_node.words[1] |= (v64 << 0) & 0x1ull;
    return mdb_node;
}

static inline void
mdb_node_ptr_set_mdbFirstBadged(mdb_node_t *mdb_node_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x1ull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node_ptr->words[1] &= ~0x1ull;
    mdb_node_ptr->words[1] |= (v64 << 0) & 0x1;
}

static inline uint64_t CONST
mdb_node_get_mdbPrev(mdb_node_t mdb_node) {
    uint64_t ret;
    ret = (mdb_node.words[0] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline mdb_node_t CONST
mdb_node_set_mdbPrev(mdb_node_t mdb_node, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node.words[0] &= ~0xffffffffffffffffull;
    mdb_node.words[0] |= (v64 << 0) & 0xffffffffffffffffull;
    return mdb_node;
}

static inline void
mdb_node_ptr_set_mdbPrev(mdb_node_t *mdb_node_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    mdb_node_ptr->words[0] &= ~0xffffffffffffffffull;
    mdb_node_ptr->words[0] |= (v64 << 0) & 0xffffffffffffffff;
}

struct notification {
    uint64_t words[4];
};
typedef struct notification notification_t;

static inline uint64_t PURE
notification_ptr_get_ntfnBoundTCB(notification_t *notification_ptr) {
    uint64_t ret;
    ret = (notification_ptr->words[3] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
notification_ptr_set_ntfnBoundTCB(notification_t *notification_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffull << 0) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    notification_ptr->words[3] &= ~0xffffffffffffull;
    notification_ptr->words[3] |= (v64 >> 0) & 0xffffffffffff;
}

static inline uint64_t PURE
notification_ptr_get_ntfnMsgIdentifier(notification_t *notification_ptr) {
    uint64_t ret;
    ret = (notification_ptr->words[2] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
notification_ptr_set_ntfnMsgIdentifier(notification_t *notification_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    notification_ptr->words[2] &= ~0xffffffffffffffffull;
    notification_ptr->words[2] |= (v64 << 0) & 0xffffffffffffffff;
}

static inline uint64_t PURE
notification_ptr_get_ntfnQueue_head(notification_t *notification_ptr) {
    uint64_t ret;
    ret = (notification_ptr->words[1] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
notification_ptr_set_ntfnQueue_head(notification_t *notification_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffull << 0) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    notification_ptr->words[1] &= ~0xffffffffffffull;
    notification_ptr->words[1] |= (v64 >> 0) & 0xffffffffffff;
}

static inline uint64_t PURE
notification_ptr_get_ntfnQueue_tail(notification_t *notification_ptr) {
    uint64_t ret;
    ret = (notification_ptr->words[0] & 0xffffffffffff0000ull) >> 16;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
notification_ptr_set_ntfnQueue_tail(notification_t *notification_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffff0000ull >> 16) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    notification_ptr->words[0] &= ~0xffffffffffff0000ull;
    notification_ptr->words[0] |= (v64 << 16) & 0xffffffffffff0000;
}

static inline uint64_t PURE
notification_ptr_get_state(notification_t *notification_ptr) {
    uint64_t ret;
    ret = (notification_ptr->words[0] & 0x3ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
notification_ptr_set_state(notification_t *notification_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x3ull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    notification_ptr->words[0] &= ~0x3ull;
    notification_ptr->words[0] |= (v64 << 0) & 0x3;
}

struct thread_state {
    uint64_t words[3];
};
typedef struct thread_state thread_state_t;

static inline uint64_t PURE
thread_state_ptr_get_blockingIPCBadge(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[2] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_blockingIPCBadge(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[2] &= ~0xffffffffffffffffull;
    thread_state_ptr->words[2] |= (v64 << 0) & 0xffffffffffffffff;
}

static inline uint64_t PURE
thread_state_ptr_get_blockingIPCCanGrant(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[1] & 0x8ull) >> 3;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_blockingIPCCanGrant(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x8ull >> 3) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[1] &= ~0x8ull;
    thread_state_ptr->words[1] |= (v64 << 3) & 0x8;
}

static inline uint64_t PURE
thread_state_ptr_get_blockingIPCCanGrantReply(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[1] & 0x4ull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_blockingIPCCanGrantReply(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x4ull >> 2) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[1] &= ~0x4ull;
    thread_state_ptr->words[1] |= (v64 << 2) & 0x4;
}

static inline uint64_t PURE
thread_state_ptr_get_blockingIPCIsCall(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[1] & 0x2ull) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_blockingIPCIsCall(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x2ull >> 1) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[1] &= ~0x2ull;
    thread_state_ptr->words[1] |= (v64 << 1) & 0x2;
}

static inline uint64_t CONST
thread_state_get_tcbQueued(thread_state_t thread_state) {
    uint64_t ret;
    ret = (thread_state.words[1] & 0x1ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_tcbQueued(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0x1ull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[1] &= ~0x1ull;
    thread_state_ptr->words[1] |= (v64 << 0) & 0x1;
}

static inline uint64_t PURE
thread_state_ptr_get_blockingObject(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[0] & 0xfffffffffff0ull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline void
thread_state_ptr_set_blockingObject(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xfffffffffff0ull << 0) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));
    thread_state_ptr->words[0] &= ~0xfffffffffff0ull;
    thread_state_ptr->words[0] |= (v64 >> 0) & 0xfffffffffff0;
}

static inline uint64_t CONST
thread_state_get_tsType(thread_state_t thread_state) {
    uint64_t ret;
    ret = (thread_state.words[0] & 0xfull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t PURE
thread_state_ptr_get_tsType(thread_state_t *thread_state_ptr) {
    uint64_t ret;
    ret = (thread_state_ptr->words[0] & 0xfull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline void
thread_state_ptr_set_tsType(thread_state_t *thread_state_ptr, uint64_t v64) {
    /* fail if user has passed bits that we will override */
    assert((((~0xfull >> 0) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));
    thread_state_ptr->words[0] &= ~0xfull;
    thread_state_ptr->words[0] |= (v64 << 0) & 0xf;
}

struct ttbr {
    uint64_t words[1];
};
typedef struct ttbr ttbr_t;

static inline ttbr_t CONST
ttbr_new(uint64_t asid, uint64_t base_address) {
    ttbr_t ttbr;

    /* fail if user has passed bits that we will override */  
    assert((asid & ~0xffffull) == ((0 && (asid & (1ull << 47))) ? 0x0 : 0));  
    assert((base_address & ~0xffffffffffffull) == ((0 && (base_address & (1ull << 47))) ? 0x0 : 0));

    ttbr.words[0] = 0
        | (asid & 0xffffull) << 48
        | (base_address & 0xffffffffffffull) >> 0;

    return ttbr;
}

struct vm_attributes {
    uint64_t words[1];
};
typedef struct vm_attributes vm_attributes_t;

static inline vm_attributes_t CONST
vm_attributes_new(uint64_t armExecuteNever, uint64_t armParityEnabled, uint64_t armPageCacheable) {
    vm_attributes_t vm_attributes;

    /* fail if user has passed bits that we will override */  
    assert((armExecuteNever & ~0x1ull) == ((1 && (armExecuteNever & (1ull << 47))) ? 0x0 : 0));  
    assert((armParityEnabled & ~0x1ull) == ((1 && (armParityEnabled & (1ull << 47))) ? 0x0 : 0));  
    assert((armPageCacheable & ~0x1ull) == ((1 && (armPageCacheable & (1ull << 47))) ? 0x0 : 0));

    vm_attributes.words[0] = 0
        | (armExecuteNever & 0x1ull) << 2
        | (armParityEnabled & 0x1ull) << 1
        | (armPageCacheable & 0x1ull) << 0;

    return vm_attributes;
}

static inline uint64_t CONST
vm_attributes_get_armExecuteNever(vm_attributes_t vm_attributes) {
    uint64_t ret;
    ret = (vm_attributes.words[0] & 0x4ull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
vm_attributes_get_armPageCacheable(vm_attributes_t vm_attributes) {
    uint64_t ret;
    ret = (vm_attributes.words[0] & 0x1ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

struct asid_map {
    uint64_t words[1];
};
typedef struct asid_map asid_map_t;

enum asid_map_tag {
    asid_map_asid_map_none = 0,
    asid_map_asid_map_vspace = 1
};
typedef enum asid_map_tag asid_map_tag_t;

static inline uint64_t CONST
asid_map_get_type(asid_map_t asid_map) {
    return (asid_map.words[0] >> 0) & 0x1ull;
}

static inline asid_map_t CONST
asid_map_asid_map_none_new(void) {
    asid_map_t asid_map;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)asid_map_asid_map_none & ~0x1ull) == ((1 && ((uint64_t)asid_map_asid_map_none & (1ull << 47))) ? 0x0 : 0));

    asid_map.words[0] = 0
        | ((uint64_t)asid_map_asid_map_none & 0x1ull) << 0;

    return asid_map;
}

static inline asid_map_t CONST
asid_map_asid_map_vspace_new(uint64_t vspace_root) {
    asid_map_t asid_map;

    /* fail if user has passed bits that we will override */  
    assert((vspace_root & ~0xfffffffff000ull) == ((1 && (vspace_root & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)asid_map_asid_map_vspace & ~0x1ull) == ((1 && ((uint64_t)asid_map_asid_map_vspace & (1ull << 47))) ? 0x0 : 0));

    asid_map.words[0] = 0
        | (vspace_root & 0xfffffffff000ull) >> 0
        | ((uint64_t)asid_map_asid_map_vspace & 0x1ull) << 0;

    return asid_map;
}

static inline uint64_t CONST
asid_map_asid_map_vspace_get_vspace_root(asid_map_t asid_map) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((asid_map.words[0] >> 0) & 0x1) == asid_map_asid_map_vspace);

    ret = (asid_map.words[0] & 0xfffffffff000ull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

struct cap {
    uint64_t words[2];
};
typedef struct cap cap_t;

enum cap_tag {
    cap_null_cap = 0,
    cap_untyped_cap = 2,
    cap_endpoint_cap = 4,
    cap_notification_cap = 6,
    cap_reply_cap = 8,
    cap_cnode_cap = 10,
    cap_thread_cap = 12,
    cap_irq_control_cap = 14,
    cap_irq_handler_cap = 16,
    cap_zombie_cap = 18,
    cap_domain_cap = 20,
    cap_frame_cap = 1,
    cap_page_table_cap = 3,
    cap_vspace_cap = 9,
    cap_asid_control_cap = 11,
    cap_asid_pool_cap = 13,
    cap_sgi_signal_cap = 27
};
typedef enum cap_tag cap_tag_t;

static inline uint64_t CONST
cap_get_capType(cap_t cap) {
    return (cap.words[0] >> 59) & 0x1full;
}

static inline int CONST
cap_capType_equals(cap_t cap, uint64_t cap_type_tag) {
    return ((cap.words[0] >> 59) & 0x1full) == cap_type_tag;
}

static inline cap_t CONST
cap_null_cap_new(void) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_null_cap & ~0x1full) == ((1 && ((uint64_t)cap_null_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_null_cap & 0x1full) << 59;
    cap.words[1] = 0;

    return cap;
}

static inline cap_t CONST
cap_untyped_cap_new(uint64_t capFreeIndex, uint64_t capIsDevice, uint64_t capBlockSize, uint64_t capPtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capFreeIndex & ~0xffffffffffffull) == ((1 && (capFreeIndex & (1ull << 47))) ? 0x0 : 0));  
    assert((capIsDevice & ~0x1ull) == ((1 && (capIsDevice & (1ull << 47))) ? 0x0 : 0));  
    assert((capBlockSize & ~0x3full) == ((1 && (capBlockSize & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)cap_untyped_cap & ~0x1full) == ((1 && ((uint64_t)cap_untyped_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capPtr & ~0xffffffffffffull) == ((1 && (capPtr & (1ull << 47))) ? 0xffff000000000000 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_untyped_cap & 0x1full) << 59
        | (capPtr & 0xffffffffffffull) >> 0;
    cap.words[1] = 0
        | (capFreeIndex & 0xffffffffffffull) << 16
        | (capIsDevice & 0x1ull) << 6
        | (capBlockSize & 0x3full) << 0;

    return cap;
}

static inline uint64_t CONST
cap_untyped_cap_get_capFreeIndex(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_untyped_cap);

    ret = (cap.words[1] & 0xffffffffffff0000ull) >> 16;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_untyped_cap_set_capFreeIndex(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_untyped_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffff0000ull >> 16 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffffffffffff0000ull;
    cap.words[1] |= (v64 << 16) & 0xffffffffffff0000ull;
    return cap;
}

static inline void
cap_untyped_cap_ptr_set_capFreeIndex(cap_t *cap_ptr, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap_ptr->words[0] >> 59) & 0x1f) == cap_untyped_cap);

    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffff0000ull >> 16) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap_ptr->words[1] &= ~0xffffffffffff0000ull;
    cap_ptr->words[1] |= (v64 << 16) & 0xffffffffffff0000ull;
}

static inline uint64_t CONST
cap_untyped_cap_get_capIsDevice(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_untyped_cap);

    ret = (cap.words[1] & 0x40ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_untyped_cap_get_capBlockSize(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_untyped_cap);

    ret = (cap.words[1] & 0x3full) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_untyped_cap_get_capPtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_untyped_cap);

    ret = (cap.words[0] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_new(uint64_t capEPBadge, uint64_t capCanGrantReply, uint64_t capCanGrant, uint64_t capCanSend, uint64_t capCanReceive, uint64_t capEPPtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capCanGrantReply & ~0x1ull) == ((1 && (capCanGrantReply & (1ull << 47))) ? 0x0 : 0));  
    assert((capCanGrant & ~0x1ull) == ((1 && (capCanGrant & (1ull << 47))) ? 0x0 : 0));  
    assert((capCanSend & ~0x1ull) == ((1 && (capCanSend & (1ull << 47))) ? 0x0 : 0));  
    assert((capCanReceive & ~0x1ull) == ((1 && (capCanReceive & (1ull << 47))) ? 0x0 : 0));  
    assert((capEPPtr & ~0xffffffffffffull) == ((1 && (capEPPtr & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)cap_endpoint_cap & ~0x1full) == ((1 && ((uint64_t)cap_endpoint_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | (capCanGrantReply & 0x1ull) << 58
        | (capCanGrant & 0x1ull) << 57
        | (capCanSend & 0x1ull) << 55
        | (capCanReceive & 0x1ull) << 56
        | (capEPPtr & 0xffffffffffffull) >> 0
        | ((uint64_t)cap_endpoint_cap & 0x1full) << 59;
    cap.words[1] = 0
        | capEPBadge << 0;

    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capEPBadge(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_set_capEPBadge(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffffffffffffffffull;
    cap.words[1] |= (v64 << 0) & 0xffffffffffffffffull;
    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capCanGrantReply(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[0] & 0x400000000000000ull) >> 58;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_set_capCanGrantReply(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x400000000000000ull >> 58 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x400000000000000ull;
    cap.words[0] |= (v64 << 58) & 0x400000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capCanGrant(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[0] & 0x200000000000000ull) >> 57;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_set_capCanGrant(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x200000000000000ull >> 57 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x200000000000000ull;
    cap.words[0] |= (v64 << 57) & 0x200000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capCanReceive(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[0] & 0x100000000000000ull) >> 56;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_set_capCanReceive(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x100000000000000ull >> 56 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x100000000000000ull;
    cap.words[0] |= (v64 << 56) & 0x100000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capCanSend(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[0] & 0x80000000000000ull) >> 55;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_endpoint_cap_set_capCanSend(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x80000000000000ull >> 55 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x80000000000000ull;
    cap.words[0] |= (v64 << 55) & 0x80000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_endpoint_cap_get_capEPPtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_endpoint_cap);

    ret = (cap.words[0] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_notification_cap_new(uint64_t capNtfnBadge, uint64_t capNtfnCanReceive, uint64_t capNtfnCanSend, uint64_t capNtfnPtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_notification_cap & ~0x1full) == ((1 && ((uint64_t)cap_notification_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capNtfnCanReceive & ~0x1ull) == ((1 && (capNtfnCanReceive & (1ull << 47))) ? 0x0 : 0));  
    assert((capNtfnCanSend & ~0x1ull) == ((1 && (capNtfnCanSend & (1ull << 47))) ? 0x0 : 0));  
    assert((capNtfnPtr & ~0xffffffffffffull) == ((1 && (capNtfnPtr & (1ull << 47))) ? 0xffff000000000000 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_notification_cap & 0x1full) << 59
        | (capNtfnCanReceive & 0x1ull) << 58
        | (capNtfnCanSend & 0x1ull) << 57
        | (capNtfnPtr & 0xffffffffffffull) >> 0;
    cap.words[1] = 0
        | capNtfnBadge << 0;

    return cap;
}

static inline uint64_t CONST
cap_notification_cap_get_capNtfnBadge(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);

    ret = (cap.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_notification_cap_set_capNtfnBadge(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffffffffffffffffull;
    cap.words[1] |= (v64 << 0) & 0xffffffffffffffffull;
    return cap;
}

static inline uint64_t CONST
cap_notification_cap_get_capNtfnCanReceive(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);

    ret = (cap.words[0] & 0x400000000000000ull) >> 58;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_notification_cap_set_capNtfnCanReceive(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x400000000000000ull >> 58 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x400000000000000ull;
    cap.words[0] |= (v64 << 58) & 0x400000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_notification_cap_get_capNtfnCanSend(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);

    ret = (cap.words[0] & 0x200000000000000ull) >> 57;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_notification_cap_set_capNtfnCanSend(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x200000000000000ull >> 57 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x200000000000000ull;
    cap.words[0] |= (v64 << 57) & 0x200000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_notification_cap_get_capNtfnPtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_notification_cap);

    ret = (cap.words[0] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_reply_cap_new(uint64_t capReplyCanGrant, uint64_t capReplyMaster, uint64_t capTCBPtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capReplyCanGrant & ~0x1ull) == ((1 && (capReplyCanGrant & (1ull << 47))) ? 0x0 : 0));  
    assert((capReplyMaster & ~0x1ull) == ((1 && (capReplyMaster & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)cap_reply_cap & ~0x1full) == ((1 && ((uint64_t)cap_reply_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | (capReplyCanGrant & 0x1ull) << 1
        | (capReplyMaster & 0x1ull) << 0
        | ((uint64_t)cap_reply_cap & 0x1full) << 59;
    cap.words[1] = 0
        | capTCBPtr << 0;

    return cap;
}

static inline uint64_t CONST
cap_reply_cap_get_capTCBPtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_reply_cap);

    ret = (cap.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_reply_cap_get_capReplyCanGrant(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_reply_cap);

    ret = (cap.words[0] & 0x2ull) >> 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_reply_cap_set_capReplyCanGrant(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_reply_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x2ull >> 1 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x2ull;
    cap.words[0] |= (v64 << 1) & 0x2ull;
    return cap;
}

static inline uint64_t CONST
cap_reply_cap_get_capReplyMaster(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_reply_cap);

    ret = (cap.words[0] & 0x1ull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_cnode_cap_new(uint64_t capCNodeRadix, uint64_t capCNodeGuardSize, uint64_t capCNodeGuard, uint64_t capCNodePtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capCNodeRadix & ~0x3full) == ((1 && (capCNodeRadix & (1ull << 47))) ? 0x0 : 0));  
    assert((capCNodeGuardSize & ~0x3full) == ((1 && (capCNodeGuardSize & (1ull << 47))) ? 0x0 : 0));  
    assert((capCNodePtr & ~0xfffffffffffeull) == ((1 && (capCNodePtr & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)cap_cnode_cap & ~0x1full) == ((1 && ((uint64_t)cap_cnode_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | (capCNodeRadix & 0x3full) << 47
        | (capCNodeGuardSize & 0x3full) << 53
        | (capCNodePtr & 0xfffffffffffeull) >> 1
        | ((uint64_t)cap_cnode_cap & 0x1full) << 59;
    cap.words[1] = 0
        | capCNodeGuard << 0;

    return cap;
}

static inline uint64_t CONST
cap_cnode_cap_get_capCNodeGuard(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);

    ret = (cap.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_cnode_cap_set_capCNodeGuard(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffffffffffffffffull;
    cap.words[1] |= (v64 << 0) & 0xffffffffffffffffull;
    return cap;
}

static inline uint64_t CONST
cap_cnode_cap_get_capCNodeGuardSize(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);

    ret = (cap.words[0] & 0x7e0000000000000ull) >> 53;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_cnode_cap_set_capCNodeGuardSize(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x7e0000000000000ull >> 53 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x7e0000000000000ull;
    cap.words[0] |= (v64 << 53) & 0x7e0000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_cnode_cap_get_capCNodeRadix(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);

    ret = (cap.words[0] & 0x1f800000000000ull) >> 47;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_cnode_cap_get_capCNodePtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_cnode_cap);

    ret = (cap.words[0] & 0x7fffffffffffull) << 1;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_thread_cap_new(uint64_t capTCBPtr) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_thread_cap & ~0x1full) == ((1 && ((uint64_t)cap_thread_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capTCBPtr & ~0xffffffffffffull) == ((1 && (capTCBPtr & (1ull << 47))) ? 0xffff000000000000 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_thread_cap & 0x1full) << 59
        | (capTCBPtr & 0xffffffffffffull) >> 0;
    cap.words[1] = 0;

    return cap;
}

static inline uint64_t CONST
cap_thread_cap_get_capTCBPtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_thread_cap);

    ret = (cap.words[0] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_irq_control_cap_new(void) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_irq_control_cap & ~0x1full) == ((1 && ((uint64_t)cap_irq_control_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_irq_control_cap & 0x1full) << 59;
    cap.words[1] = 0;

    return cap;
}

static inline cap_t CONST
cap_irq_handler_cap_new(uint64_t capIRQ) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capIRQ & ~0xfffull) == ((1 && (capIRQ & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)cap_irq_handler_cap & ~0x1full) == ((1 && ((uint64_t)cap_irq_handler_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_irq_handler_cap & 0x1full) << 59;
    cap.words[1] = 0
        | (capIRQ & 0xfffull) << 0;

    return cap;
}

static inline uint64_t CONST
cap_irq_handler_cap_get_capIRQ(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_irq_handler_cap);

    ret = (cap.words[1] & 0xfffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_zombie_cap_new(uint64_t capZombieID, uint64_t capZombieType) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_zombie_cap & ~0x1full) == ((1 && ((uint64_t)cap_zombie_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capZombieType & ~0x7full) == ((1 && (capZombieType & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_zombie_cap & 0x1full) << 59
        | (capZombieType & 0x7full) << 0;
    cap.words[1] = 0
        | capZombieID << 0;

    return cap;
}

static inline uint64_t CONST
cap_zombie_cap_get_capZombieID(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_zombie_cap);

    ret = (cap.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_zombie_cap_set_capZombieID(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_zombie_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffffffffffffffffull >> 0 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffffffffffffffffull;
    cap.words[1] |= (v64 << 0) & 0xffffffffffffffffull;
    return cap;
}

static inline uint64_t CONST
cap_zombie_cap_get_capZombieType(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_zombie_cap);

    ret = (cap.words[0] & 0x7full) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_domain_cap_new(void) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_domain_cap & ~0x1full) == ((1 && ((uint64_t)cap_domain_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_domain_cap & 0x1full) << 59;
    cap.words[1] = 0;

    return cap;
}

static inline cap_t CONST
cap_frame_cap_new(uint64_t capFMappedASID, uint64_t capFBasePtr, uint64_t capFSize, uint64_t capFMappedAddress, uint64_t capFVMRights, uint64_t capFIsDevice) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capFMappedASID & ~0xffffull) == ((1 && (capFMappedASID & (1ull << 47))) ? 0x0 : 0));  
    assert((capFBasePtr & ~0xffffffffffffull) == ((1 && (capFBasePtr & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)cap_frame_cap & ~0x1full) == ((1 && ((uint64_t)cap_frame_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capFSize & ~0x3ull) == ((1 && (capFSize & (1ull << 47))) ? 0x0 : 0));  
    assert((capFMappedAddress & ~0xffffffffffffull) == ((1 && (capFMappedAddress & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert((capFVMRights & ~0x3ull) == ((1 && (capFVMRights & (1ull << 47))) ? 0x0 : 0));  
    assert((capFIsDevice & ~0x1ull) == ((1 && (capFIsDevice & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_frame_cap & 0x1full) << 59
        | (capFSize & 0x3ull) << 57
        | (capFMappedAddress & 0xffffffffffffull) << 9
        | (capFVMRights & 0x3ull) << 7
        | (capFIsDevice & 0x1ull) << 6;
    cap.words[1] = 0
        | (capFMappedASID & 0xffffull) << 48
        | (capFBasePtr & 0xffffffffffffull) >> 0;

    return cap;
}

static inline uint64_t CONST
cap_frame_cap_get_capFMappedASID(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[1] & 0xffff000000000000ull) >> 48;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_frame_cap_set_capFMappedASID(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffff000000000000ull >> 48 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffff000000000000ull;
    cap.words[1] |= (v64 << 48) & 0xffff000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_frame_cap_get_capFBasePtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[1] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline uint64_t CONST
cap_frame_cap_get_capFSize(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[0] & 0x600000000000000ull) >> 57;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_frame_cap_get_capFMappedAddress(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[0] & 0x1fffffffffffe00ull) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_frame_cap_set_capFMappedAddress(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x1fffffffffffe00ull >> 9 ) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));

    cap.words[0] &= ~0x1fffffffffffe00ull;
    cap.words[0] |= (v64 << 9) & 0x1fffffffffffe00ull;
    return cap;
}

static inline uint64_t CONST
cap_frame_cap_get_capFVMRights(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[0] & 0x180ull) >> 7;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_frame_cap_set_capFVMRights(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x180ull >> 7 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x180ull;
    cap.words[0] |= (v64 << 7) & 0x180ull;
    return cap;
}

static inline uint64_t CONST
cap_frame_cap_get_capFIsDevice(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_frame_cap);

    ret = (cap.words[0] & 0x40ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_page_table_cap_new(uint64_t capPTMappedASID, uint64_t capPTBasePtr, uint64_t capPTIsMapped, uint64_t capPTMappedAddress) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capPTMappedASID & ~0xffffull) == ((1 && (capPTMappedASID & (1ull << 47))) ? 0x0 : 0));  
    assert((capPTBasePtr & ~0xffffffffffffull) == ((1 && (capPTBasePtr & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)cap_page_table_cap & ~0x1full) == ((1 && ((uint64_t)cap_page_table_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capPTIsMapped & ~0x1ull) == ((1 && (capPTIsMapped & (1ull << 47))) ? 0x0 : 0));  
    assert((capPTMappedAddress & ~0xfffffff00000ull) == ((1 && (capPTMappedAddress & (1ull << 47))) ? 0xffff000000000000 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_page_table_cap & 0x1full) << 59
        | (capPTIsMapped & 0x1ull) << 48
        | (capPTMappedAddress & 0xfffffff00000ull) >> 0;
    cap.words[1] = 0
        | (capPTMappedASID & 0xffffull) << 48
        | (capPTBasePtr & 0xffffffffffffull) >> 0;

    return cap;
}

static inline uint64_t CONST
cap_page_table_cap_get_capPTMappedASID(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);

    ret = (cap.words[1] & 0xffff000000000000ull) >> 48;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_page_table_cap_set_capPTMappedASID(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffff000000000000ull >> 48 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffff000000000000ull;
    cap.words[1] |= (v64 << 48) & 0xffff000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_page_table_cap_get_capPTBasePtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);

    ret = (cap.words[1] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline uint64_t CONST
cap_page_table_cap_get_capPTIsMapped(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);

    ret = (cap.words[0] & 0x1000000000000ull) >> 48;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_page_table_cap_set_capPTIsMapped(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x1000000000000ull >> 48 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x1000000000000ull;
    cap.words[0] |= (v64 << 48) & 0x1000000000000ull;
    return cap;
}

static inline void
cap_page_table_cap_ptr_set_capPTIsMapped(cap_t *cap_ptr, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap_ptr->words[0] >> 59) & 0x1f) == cap_page_table_cap);

    /* fail if user has passed bits that we will override */
    assert((((~0x1000000000000ull >> 48) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap_ptr->words[0] &= ~0x1000000000000ull;
    cap_ptr->words[0] |= (v64 << 48) & 0x1000000000000ull;
}

static inline uint64_t CONST
cap_page_table_cap_get_capPTMappedAddress(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);

    ret = (cap.words[0] & 0xfffffff00000ull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_page_table_cap_set_capPTMappedAddress(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_page_table_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xfffffff00000ull << 0 ) | 0xffff000000000000) & v64) == ((1 && (v64 & (1ull << (47)))) ? 0xffff000000000000 : 0));

    cap.words[0] &= ~0xfffffff00000ull;
    cap.words[0] |= (v64 >> 0) & 0xfffffff00000ull;
    return cap;
}

static inline cap_t CONST
cap_vspace_cap_new(uint64_t capVSMappedASID, uint64_t capVSBasePtr, uint64_t capVSIsMapped) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capVSMappedASID & ~0xffffull) == ((1 && (capVSMappedASID & (1ull << 47))) ? 0x0 : 0));  
    assert((capVSBasePtr & ~0xffffffffffffull) == ((1 && (capVSBasePtr & (1ull << 47))) ? 0xffff000000000000 : 0));  
    assert(((uint64_t)cap_vspace_cap & ~0x1full) == ((1 && ((uint64_t)cap_vspace_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capVSIsMapped & ~0x1ull) == ((1 && (capVSIsMapped & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_vspace_cap & 0x1full) << 59
        | (capVSIsMapped & 0x1ull) << 58;
    cap.words[1] = 0
        | (capVSMappedASID & 0xffffull) << 48
        | (capVSBasePtr & 0xffffffffffffull) >> 0;

    return cap;
}

static inline uint64_t CONST
cap_vspace_cap_get_capVSMappedASID(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_vspace_cap);

    ret = (cap.words[1] & 0xffff000000000000ull) >> 48;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_vspace_cap_set_capVSMappedASID(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_vspace_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0xffff000000000000ull >> 48 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[1] &= ~0xffff000000000000ull;
    cap.words[1] |= (v64 << 48) & 0xffff000000000000ull;
    return cap;
}

static inline uint64_t CONST
cap_vspace_cap_get_capVSBasePtr(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_vspace_cap);

    ret = (cap.words[1] & 0xffffffffffffull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline uint64_t CONST
cap_vspace_cap_get_capVSIsMapped(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_vspace_cap);

    ret = (cap.words[0] & 0x400000000000000ull) >> 58;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline cap_t CONST
cap_vspace_cap_set_capVSIsMapped(cap_t cap, uint64_t v64) {
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_vspace_cap);
    /* fail if user has passed bits that we will override */
    assert((((~0x400000000000000ull >> 58 ) | 0x0) & v64) == ((0 && (v64 & (1ull << (47)))) ? 0x0 : 0));

    cap.words[0] &= ~0x400000000000000ull;
    cap.words[0] |= (v64 << 58) & 0x400000000000000ull;
    return cap;
}

static inline cap_t CONST
cap_asid_control_cap_new(void) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_asid_control_cap & ~0x1full) == ((1 && ((uint64_t)cap_asid_control_cap & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_asid_control_cap & 0x1full) << 59;
    cap.words[1] = 0;

    return cap;
}

static inline cap_t CONST
cap_asid_pool_cap_new(uint64_t capASIDBase, uint64_t capASIDPool) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)cap_asid_pool_cap & ~0x1full) == ((1 && ((uint64_t)cap_asid_pool_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capASIDBase & ~0xffffull) == ((1 && (capASIDBase & (1ull << 47))) ? 0x0 : 0));  
    assert((capASIDPool & ~0xfffffffff800ull) == ((1 && (capASIDPool & (1ull << 47))) ? 0xffff000000000000 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_asid_pool_cap & 0x1full) << 59
        | (capASIDBase & 0xffffull) << 43
        | (capASIDPool & 0xfffffffff800ull) >> 11;
    cap.words[1] = 0;

    return cap;
}

static inline uint64_t CONST
cap_asid_pool_cap_get_capASIDBase(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_asid_pool_cap);

    ret = (cap.words[0] & 0x7fff80000000000ull) >> 43;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_asid_pool_cap_get_capASIDPool(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_asid_pool_cap);

    ret = (cap.words[0] & 0x1fffffffffull) << 11;
    /* Possibly sign extend */
    if (__builtin_expect(!!(1 && (ret & (1ull << (47)))), 1)) {
        ret |= 0xffff000000000000;
    }
    return ret;
}

static inline cap_t CONST
cap_sgi_signal_cap_new(uint64_t capSGITarget, uint64_t capSGIIRQ) {
    cap_t cap;

    /* fail if user has passed bits that we will override */  
    assert((capSGITarget & ~0xffffffffull) == ((1 && (capSGITarget & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)cap_sgi_signal_cap & ~0x1full) == ((1 && ((uint64_t)cap_sgi_signal_cap & (1ull << 47))) ? 0x0 : 0));  
    assert((capSGIIRQ & ~0xfull) == ((1 && (capSGIIRQ & (1ull << 47))) ? 0x0 : 0));

    cap.words[0] = 0
        | ((uint64_t)cap_sgi_signal_cap & 0x1full) << 59
        | (capSGIIRQ & 0xfull) << 55;
    cap.words[1] = 0
        | (capSGITarget & 0xffffffffull) << 0;

    return cap;
}

static inline uint64_t CONST
cap_sgi_signal_cap_get_capSGITarget(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_sgi_signal_cap);

    ret = (cap.words[1] & 0xffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
cap_sgi_signal_cap_get_capSGIIRQ(cap_t cap) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((cap.words[0] >> 59) & 0x1f) == cap_sgi_signal_cap);

    ret = (cap.words[0] & 0x780000000000000ull) >> 55;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

struct lookup_fault {
    uint64_t words[2];
};
typedef struct lookup_fault lookup_fault_t;

enum lookup_fault_tag {
    lookup_fault_invalid_root = 0,
    lookup_fault_missing_capability = 1,
    lookup_fault_depth_mismatch = 2,
    lookup_fault_guard_mismatch = 3
};
typedef enum lookup_fault_tag lookup_fault_tag_t;

static inline uint64_t CONST
lookup_fault_get_lufType(lookup_fault_t lookup_fault) {
    return (lookup_fault.words[0] >> 0) & 0x3ull;
}

static inline lookup_fault_t CONST
lookup_fault_invalid_root_new(void) {
    lookup_fault_t lookup_fault;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)lookup_fault_invalid_root & ~0x3ull) == ((1 && ((uint64_t)lookup_fault_invalid_root & (1ull << 47))) ? 0x0 : 0));

    lookup_fault.words[0] = 0
        | ((uint64_t)lookup_fault_invalid_root & 0x3ull) << 0;
    lookup_fault.words[1] = 0;

    return lookup_fault;
}

static inline lookup_fault_t CONST
lookup_fault_missing_capability_new(uint64_t bitsLeft) {
    lookup_fault_t lookup_fault;

    /* fail if user has passed bits that we will override */  
    assert((bitsLeft & ~0x7full) == ((1 && (bitsLeft & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)lookup_fault_missing_capability & ~0x3ull) == ((1 && ((uint64_t)lookup_fault_missing_capability & (1ull << 47))) ? 0x0 : 0));

    lookup_fault.words[0] = 0
        | (bitsLeft & 0x7full) << 2
        | ((uint64_t)lookup_fault_missing_capability & 0x3ull) << 0;
    lookup_fault.words[1] = 0;

    return lookup_fault;
}

static inline uint64_t CONST
lookup_fault_missing_capability_get_bitsLeft(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_missing_capability);

    ret = (lookup_fault.words[0] & 0x1fcull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline lookup_fault_t CONST
lookup_fault_depth_mismatch_new(uint64_t bitsFound, uint64_t bitsLeft) {
    lookup_fault_t lookup_fault;

    /* fail if user has passed bits that we will override */  
    assert((bitsFound & ~0x7full) == ((1 && (bitsFound & (1ull << 47))) ? 0x0 : 0));  
    assert((bitsLeft & ~0x7full) == ((1 && (bitsLeft & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)lookup_fault_depth_mismatch & ~0x3ull) == ((1 && ((uint64_t)lookup_fault_depth_mismatch & (1ull << 47))) ? 0x0 : 0));

    lookup_fault.words[0] = 0
        | (bitsFound & 0x7full) << 9
        | (bitsLeft & 0x7full) << 2
        | ((uint64_t)lookup_fault_depth_mismatch & 0x3ull) << 0;
    lookup_fault.words[1] = 0;

    return lookup_fault;
}

static inline uint64_t CONST
lookup_fault_depth_mismatch_get_bitsFound(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_depth_mismatch);

    ret = (lookup_fault.words[0] & 0xfe00ull) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
lookup_fault_depth_mismatch_get_bitsLeft(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_depth_mismatch);

    ret = (lookup_fault.words[0] & 0x1fcull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline lookup_fault_t CONST
lookup_fault_guard_mismatch_new(uint64_t guardFound, uint64_t bitsLeft, uint64_t bitsFound) {
    lookup_fault_t lookup_fault;

    /* fail if user has passed bits that we will override */  
    assert((bitsLeft & ~0x7full) == ((1 && (bitsLeft & (1ull << 47))) ? 0x0 : 0));  
    assert((bitsFound & ~0x7full) == ((1 && (bitsFound & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)lookup_fault_guard_mismatch & ~0x3ull) == ((1 && ((uint64_t)lookup_fault_guard_mismatch & (1ull << 47))) ? 0x0 : 0));

    lookup_fault.words[0] = 0
        | (bitsLeft & 0x7full) << 9
        | (bitsFound & 0x7full) << 2
        | ((uint64_t)lookup_fault_guard_mismatch & 0x3ull) << 0;
    lookup_fault.words[1] = 0
        | guardFound << 0;

    return lookup_fault;
}

static inline uint64_t CONST
lookup_fault_guard_mismatch_get_guardFound(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_guard_mismatch);

    ret = (lookup_fault.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
lookup_fault_guard_mismatch_get_bitsLeft(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_guard_mismatch);

    ret = (lookup_fault.words[0] & 0xfe00ull) >> 9;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
lookup_fault_guard_mismatch_get_bitsFound(lookup_fault_t lookup_fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((lookup_fault.words[0] >> 0) & 0x3) == lookup_fault_guard_mismatch);

    ret = (lookup_fault.words[0] & 0x1fcull) >> 2;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

struct pte {
    uint64_t words[1];
};
typedef struct pte pte_t;

enum pte_tag {
    pte_pte_table = 3,
    pte_pte_page = 1,
    pte_pte_4k_page = 7,
    pte_pte_invalid = 0
};
typedef enum pte_tag pte_tag_t;

static inline uint64_t CONST
pte_get_pte_type(pte_t pte) {
    return ((pte.words[0] & (0x3ull << 0)) >> 0)
        | ((pte.words[0] & (0x1ull << 58)) >> 56);
}

static inline uint64_t PURE
pte_ptr_get_pte_type(pte_t *pte_ptr) {
    return ((pte_ptr->words[0] & (0x3ull << 0)) >> 0)
        | ((pte_ptr->words[0] & (0x1ull << 58)) >> 56);
}

static inline pte_t CONST
pte_pte_table_new(uint64_t pt_base_address) {
    pte_t pte;

    /* fail if user has passed bits that we will override */  
    assert((pt_base_address & ~0xfffffffff000ull) == ((0 && (pt_base_address & (1ull << 47))) ? 0x0 : 0));

    pte.words[0] = 0
        | (pt_base_address & 0xfffffffff000ull) >> 0
        | 0x3ull << 0;

    return pte;
}

static inline uint64_t PURE
pte_pte_table_ptr_get_pt_base_address(pte_t *pte_ptr) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte_ptr->words[0] >> 0) & 0x400000000000003) == 0x3ull /* sliced tag pte_pte_table */);

    ret = (pte_ptr->words[0] & 0xfffffffff000ull) << 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline pte_t CONST
pte_pte_page_new(uint64_t UXN, uint64_t page_base_address, uint64_t nG, uint64_t AF, uint64_t SH, uint64_t AP, uint64_t AttrIndx) {
    pte_t pte;

    /* fail if user has passed bits that we will override */  
    assert((UXN & ~0x1ull) == ((0 && (UXN & (1ull << 47))) ? 0x0 : 0));  
    assert((page_base_address & ~0xfffffffff000ull) == ((0 && (page_base_address & (1ull << 47))) ? 0x0 : 0));  
    assert((nG & ~0x1ull) == ((0 && (nG & (1ull << 47))) ? 0x0 : 0));  
    assert((AF & ~0x1ull) == ((0 && (AF & (1ull << 47))) ? 0x0 : 0));  
    assert((SH & ~0x3ull) == ((0 && (SH & (1ull << 47))) ? 0x0 : 0));  
    assert((AP & ~0x3ull) == ((0 && (AP & (1ull << 47))) ? 0x0 : 0));  
    assert((AttrIndx & ~0x7ull) == ((0 && (AttrIndx & (1ull << 47))) ? 0x0 : 0));

    pte.words[0] = 0
        | (UXN & 0x1ull) << 54
        | (page_base_address & 0xfffffffff000ull) >> 0
        | (nG & 0x1ull) << 11
        | (AF & 0x1ull) << 10
        | (SH & 0x3ull) << 8
        | (AP & 0x3ull) << 6
        | (AttrIndx & 0x7ull) << 2
        | 0x1ull << 0;

    return pte;
}

static inline uint64_t PURE
pte_pte_page_ptr_get_UXN(pte_t *pte_ptr) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte_ptr->words[0] >> 0) & 0x400000000000003) == 0x1ull /* sliced tag pte_pte_page */);

    ret = (pte_ptr->words[0] & 0x40000000000000ull) >> 54;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t PURE
pte_pte_page_ptr_get_SH(pte_t *pte_ptr) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte_ptr->words[0] >> 0) & 0x400000000000003) == 0x1ull /* sliced tag pte_pte_page */);

    ret = (pte_ptr->words[0] & 0x300ull) >> 8;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
pte_pte_page_get_AP(pte_t pte) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte.words[0] >> 0) & 0x400000000000003) == 0x1ull /* sliced tag pte_pte_page */);

    ret = (pte.words[0] & 0xc0ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t PURE
pte_pte_page_ptr_get_AP(pte_t *pte_ptr) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte_ptr->words[0] >> 0) & 0x400000000000003) == 0x1ull /* sliced tag pte_pte_page */);

    ret = (pte_ptr->words[0] & 0xc0ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline pte_t CONST
pte_pte_4k_page_new(uint64_t UXN, uint64_t page_base_address, uint64_t nG, uint64_t AF, uint64_t SH, uint64_t AP, uint64_t AttrIndx) {
    pte_t pte;

    /* fail if user has passed bits that we will override */  
    assert((UXN & ~0x1ull) == ((0 && (UXN & (1ull << 47))) ? 0x0 : 0));  
    assert((page_base_address & ~0xfffffffff000ull) == ((0 && (page_base_address & (1ull << 47))) ? 0x0 : 0));  
    assert((nG & ~0x1ull) == ((0 && (nG & (1ull << 47))) ? 0x0 : 0));  
    assert((AF & ~0x1ull) == ((0 && (AF & (1ull << 47))) ? 0x0 : 0));  
    assert((SH & ~0x3ull) == ((0 && (SH & (1ull << 47))) ? 0x0 : 0));  
    assert((AP & ~0x3ull) == ((0 && (AP & (1ull << 47))) ? 0x0 : 0));  
    assert((AttrIndx & ~0x7ull) == ((0 && (AttrIndx & (1ull << 47))) ? 0x0 : 0));

    pte.words[0] = 0
        | (UXN & 0x1ull) << 54
        | (page_base_address & 0xfffffffff000ull) >> 0
        | (nG & 0x1ull) << 11
        | (AF & 0x1ull) << 10
        | (SH & 0x3ull) << 8
        | (AP & 0x3ull) << 6
        | (AttrIndx & 0x7ull) << 2
        | 0x400000000000003ull << 0;

    return pte;
}

static inline uint64_t CONST
pte_pte_4k_page_get_AP(pte_t pte) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((pte.words[0] >> 0) & 0x400000000000003) == 0x400000000000003ull /* sliced tag pte_pte_4k_page */);

    ret = (pte.words[0] & 0xc0ull) >> 6;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline pte_t CONST
pte_pte_invalid_new(void) {
    pte_t pte;

    /* fail if user has passed bits that we will override */

    pte.words[0] = 0
        | 0x0ull << 0;

    return pte;
}

struct seL4_Fault {
    uint64_t words[2];
};
typedef struct seL4_Fault seL4_Fault_t;

enum seL4_Fault_tag {
    seL4_Fault_NullFault = 0,
    seL4_Fault_CapFault = 1,
    seL4_Fault_UnknownSyscall = 2,
    seL4_Fault_UserException = 3,
    seL4_Fault_VMFault = 5
};
typedef enum seL4_Fault_tag seL4_Fault_tag_t;

static inline uint64_t CONST
seL4_Fault_get_seL4_FaultType(seL4_Fault_t seL4_Fault) {
    return (seL4_Fault.words[0] >> 0) & 0xfull;
}

static inline seL4_Fault_t CONST
seL4_Fault_NullFault_new(void) {
    seL4_Fault_t seL4_Fault;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)seL4_Fault_NullFault & ~0xfull) == ((0 && ((uint64_t)seL4_Fault_NullFault & (1ull << 47))) ? 0x0 : 0));

    seL4_Fault.words[0] = 0
        | ((uint64_t)seL4_Fault_NullFault & 0xfull) << 0;
    seL4_Fault.words[1] = 0;

    return seL4_Fault;
}

static inline seL4_Fault_t CONST
seL4_Fault_CapFault_new(uint64_t address, uint64_t inReceivePhase) {
    seL4_Fault_t seL4_Fault;

    /* fail if user has passed bits that we will override */  
    assert((inReceivePhase & ~0x1ull) == ((0 && (inReceivePhase & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)seL4_Fault_CapFault & ~0xfull) == ((0 && ((uint64_t)seL4_Fault_CapFault & (1ull << 47))) ? 0x0 : 0));

    seL4_Fault.words[0] = 0
        | (inReceivePhase & 0x1ull) << 63
        | ((uint64_t)seL4_Fault_CapFault & 0xfull) << 0;
    seL4_Fault.words[1] = 0
        | address << 0;

    return seL4_Fault;
}

static inline uint64_t CONST
seL4_Fault_CapFault_get_address(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_CapFault);

    ret = (seL4_Fault.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_Fault_CapFault_get_inReceivePhase(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_CapFault);

    ret = (seL4_Fault.words[0] & 0x8000000000000000ull) >> 63;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_Fault_t CONST
seL4_Fault_UnknownSyscall_new(uint64_t syscallNumber) {
    seL4_Fault_t seL4_Fault;

    /* fail if user has passed bits that we will override */  
    assert(((uint64_t)seL4_Fault_UnknownSyscall & ~0xfull) == ((0 && ((uint64_t)seL4_Fault_UnknownSyscall & (1ull << 47))) ? 0x0 : 0));

    seL4_Fault.words[0] = 0
        | ((uint64_t)seL4_Fault_UnknownSyscall & 0xfull) << 0;
    seL4_Fault.words[1] = 0
        | syscallNumber << 0;

    return seL4_Fault;
}

static inline uint64_t CONST
seL4_Fault_UnknownSyscall_get_syscallNumber(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_UnknownSyscall);

    ret = (seL4_Fault.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_Fault_t CONST
seL4_Fault_UserException_new(uint64_t number, uint64_t code) {
    seL4_Fault_t seL4_Fault;

    /* fail if user has passed bits that we will override */  
    assert((number & ~0xffffffffull) == ((0 && (number & (1ull << 47))) ? 0x0 : 0));  
    assert((code & ~0xfffffffull) == ((0 && (code & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)seL4_Fault_UserException & ~0xfull) == ((0 && ((uint64_t)seL4_Fault_UserException & (1ull << 47))) ? 0x0 : 0));

    seL4_Fault.words[0] = 0
        | (number & 0xffffffffull) << 32
        | (code & 0xfffffffull) << 4
        | ((uint64_t)seL4_Fault_UserException & 0xfull) << 0;
    seL4_Fault.words[1] = 0;

    return seL4_Fault;
}

static inline uint64_t CONST
seL4_Fault_UserException_get_number(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_UserException);

    ret = (seL4_Fault.words[0] & 0xffffffff00000000ull) >> 32;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_Fault_UserException_get_code(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_UserException);

    ret = (seL4_Fault.words[0] & 0xfffffff0ull) >> 4;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline seL4_Fault_t CONST
seL4_Fault_VMFault_new(uint64_t address, uint64_t FSR, uint64_t instructionFault) {
    seL4_Fault_t seL4_Fault;

    /* fail if user has passed bits that we will override */  
    assert((FSR & ~0xffffffffull) == ((0 && (FSR & (1ull << 47))) ? 0x0 : 0));  
    assert((instructionFault & ~0x1ull) == ((0 && (instructionFault & (1ull << 47))) ? 0x0 : 0));  
    assert(((uint64_t)seL4_Fault_VMFault & ~0xfull) == ((0 && ((uint64_t)seL4_Fault_VMFault & (1ull << 47))) ? 0x0 : 0));

    seL4_Fault.words[0] = 0
        | (FSR & 0xffffffffull) << 32
        | (instructionFault & 0x1ull) << 31
        | ((uint64_t)seL4_Fault_VMFault & 0xfull) << 0;
    seL4_Fault.words[1] = 0
        | address << 0;

    return seL4_Fault;
}

static inline uint64_t CONST
seL4_Fault_VMFault_get_address(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_VMFault);

    ret = (seL4_Fault.words[1] & 0xffffffffffffffffull) >> 0;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_Fault_VMFault_get_FSR(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_VMFault);

    ret = (seL4_Fault.words[0] & 0xffffffff00000000ull) >> 32;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

static inline uint64_t CONST
seL4_Fault_VMFault_get_instructionFault(seL4_Fault_t seL4_Fault) {
    uint64_t ret;
    /* fail if union does not have the expected tag */
    assert(((seL4_Fault.words[0] >> 0) & 0xf) == seL4_Fault_VMFault);

    ret = (seL4_Fault.words[0] & 0x80000000ull) >> 31;
    /* Possibly sign extend */
    if (__builtin_expect(!!(0 && (ret & (1ull << (47)))), 0)) {
        ret |= 0x0;
    }
    return ret;
}

