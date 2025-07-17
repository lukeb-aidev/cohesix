// CLASSIFICATION: COMMUNITY
// Filename: wrapper.h v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

typedef unsigned long long seL4_Word;
typedef unsigned char uint8_t;

static inline void seL4_DebugPutChar(uint8_t c) { (void)c; }
static inline void seL4_Yield(void) {}
static inline void seL4_DebugHalt(void) {}
