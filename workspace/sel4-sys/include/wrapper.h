// CLASSIFICATION: COMMUNITY
// Filename: wrapper.h v1.45
// Author: Lukas Bower
// Date Modified: 2025-07-21

#include <autoconf.h>
#include <sel4/sel4/config.h>
#ifndef seL4_WordSizeBits
#define seL4_WordSizeBits CONFIG_WORD_SIZE
#endif
#include <sel4/sel4.h>
#include <sel4/config.h>
#include <sel4/gen_config.h>
#include <sel4/constants.h>
#include <sel4/syscall.h>
#include <sel4/sel4_arch/constants.h>
#include "stub_types.h"
