// CLASSIFICATION: COMMUNITY
// Filename: config.h v0.1
// Author: Lukas Bower
// Date Modified: 2028-08-30

#ifndef COHESIX_SEL4_CONFIG_H
#define COHESIX_SEL4_CONFIG_H

#define CONFIG_PRINTING 1
#define CONFIG_DEBUG_BUILD 1
#define CONFIG_DANGEROUS_CODE_INJECTION 0
#define CONFIG_ENABLE_BENCHMARKS 0
#define CONFIG_BENCHMARK_TRACK_UTILISATION 0
#define CONFIG_KERNEL_X86_DANGEROUS_MSR 0
#define CONFIG_VTX 0
#define CONFIG_SET_TLS_BASE_SELF 1

#define SEL4_FORCE_LONG_ENUM(x)

typedef unsigned long long seL4_Word;
typedef unsigned long long seL4_Uint64;
typedef unsigned int seL4_Uint32;
typedef seL4_Word seL4_CPtr;
typedef seL4_Word seL4_MessageInfo_t;

#endif // COHESIX_SEL4_CONFIG_H
