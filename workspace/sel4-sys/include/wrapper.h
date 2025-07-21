// CLASSIFICATION: COMMUNITY
// Filename: wrapper.h v1.45
// Author: Lukas Bower
// Date Modified: 2028-11-08

#pragma once

// Bring in generated CONFIG_ macros:
#include <autoconf.h>
#ifndef seL4_WordSizeBits
#define seL4_WordSizeBits CONFIG_WORD_SIZE
#endif

// Core seL4 API:
#include <sel4/sel4.h>
