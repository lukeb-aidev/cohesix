// CLASSIFICATION: COMMUNITY
// Filename: wrapper.h v1.48
// Author: Lukas Bower
// Date Modified: 2028-11-12

#pragma once

// 1) Generated config macros for kernel and libsel4:
#include <libsel4_autoconf.h>
#include <autoconf.h>
#ifndef seL4_WordSizeBits
#define seL4_WordSizeBits CONFIG_WORD_SIZE
#endif

// 2) Core seL4 API and fundamental types
#include <sel4/sel4.h>
#include <sel4/types.h>

// 3) IPC and message definitions
#include <sel4/ipc.h>
#include <sel4/messageinfo.h>

// 4) Client-side API
#include <sel4_client.h>
