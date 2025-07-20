// CLASSIFICATION: COMMUNITY
// Filename: stub_types.h v0.1
// Author: OpenAI
// Date Modified: 2028-09-10

#pragma once
#ifndef seL4_Word
typedef unsigned long long seL4_Word;
#endif
#ifndef seL4_CPtr
typedef seL4_Word seL4_CPtr;
#endif

#ifndef seL4_CNode
typedef seL4_CPtr seL4_CNode;
#endif
#ifndef seL4_IRQHandler
typedef seL4_CPtr seL4_IRQHandler;
#endif
#ifndef seL4_IRQControl
typedef seL4_CPtr seL4_IRQControl;
#endif
#ifndef seL4_TCB
typedef seL4_CPtr seL4_TCB;
#endif
#ifndef seL4_Untyped
typedef seL4_CPtr seL4_Untyped;
#endif
#ifndef seL4_DomainSet
typedef seL4_CPtr seL4_DomainSet;
#endif
#ifndef seL4_SchedContext
typedef seL4_CPtr seL4_SchedContext;
#endif
#ifndef seL4_SchedControl
typedef seL4_CPtr seL4_SchedControl;
#endif
#ifndef seL4_ARM_VMAttributes
typedef seL4_CPtr seL4_ARM_VMAttributes;
#endif
#ifndef seL4_ARM_Page
typedef seL4_CPtr seL4_ARM_Page;
#endif
#ifndef seL4_ARM_PageTable
typedef seL4_CPtr seL4_ARM_PageTable;
#endif
#ifndef seL4_ARM_VSpace
typedef seL4_CPtr seL4_ARM_VSpace;
#endif
#ifndef seL4_ARM_ASIDControl
typedef seL4_CPtr seL4_ARM_ASIDControl;
#endif
#ifndef seL4_ARM_ASIDPool
typedef seL4_CPtr seL4_ARM_ASIDPool;
#endif
#ifndef seL4_ARM_VCPU
typedef seL4_CPtr seL4_ARM_VCPU;
#endif
#ifndef seL4_ARM_IOSpace
typedef seL4_CPtr seL4_ARM_IOSpace;
#endif
#ifndef seL4_ARM_IOPageTable
typedef seL4_CPtr seL4_ARM_IOPageTable;
#endif
#ifndef seL4_ARM_SMC
typedef seL4_CPtr seL4_ARM_SMC;
#endif
#ifndef seL4_UserContext
typedef struct { seL4_Word words[36]; } seL4_UserContext;
#endif
#ifndef seL4_ARM_SMCContext
typedef struct { seL4_Word words[8]; } seL4_ARM_SMCContext;
#endif
#ifndef seL4_VCPUReg
typedef seL4_Word seL4_VCPUReg;
#endif
#ifndef seL4_ARM_SIDControl
typedef seL4_CPtr seL4_ARM_SIDControl;
#endif
#ifndef seL4_ARM_SID
typedef seL4_CPtr seL4_ARM_SID;
#endif
#ifndef seL4_ARM_CBControl
typedef seL4_CPtr seL4_ARM_CBControl;
#endif
#ifndef seL4_ARM_CB
typedef seL4_CPtr seL4_ARM_CB;
#endif
