// CLASSIFICATION: COMMUNITY
// Filename: wrapper.h v1.47
// Author: Lukas Bower
// Date Modified: 2028-11-11

#pragma once

// 1) Configuration macros (CONFIG_WORD_SIZE, etc.)
#include <autoconf.h>
#ifndef seL4_WordSizeBits
#define seL4_WordSizeBits CONFIG_WORD_SIZE
#endif

// 2) Core seL4 API
#include <sel4/sel4.h>


#include <sel4_client.h>
// 3) Additional top-level headers if needed...
