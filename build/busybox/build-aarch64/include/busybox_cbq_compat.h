#ifndef BUSYBOX_CBQ_COMPAT_H
#define BUSYBOX_CBQ_COMPAT_H

#include <linux/types.h>

/* CBQ compatibility header providing necessary structures and
 * constants when kernel headers omit them. These definitions are
 * taken from Linux v5.15 include/uapi/linux/pkt_sched.h. */

#ifndef TC_CBQ_MAXPRIO
#define TC_CBQ_MAXPRIO          8
#endif
#ifndef TC_CBQ_MAXLEVEL
#define TC_CBQ_MAXLEVEL         8
#endif
#ifndef TC_CBQ_DEF_EWMA
#define TC_CBQ_DEF_EWMA         5
#endif

#ifndef TCF_CBQ_LSS_BOUNDED
struct tc_cbq_lssopt {
        unsigned char   change;
        unsigned char   flags;
#define TCF_CBQ_LSS_BOUNDED     1
#define TCF_CBQ_LSS_ISOLATED    2
        unsigned char   ewma_log;
        unsigned char   level;
#define TCF_CBQ_LSS_FLAGS       1
#define TCF_CBQ_LSS_EWMA        2
#define TCF_CBQ_LSS_MAXIDLE     4
#define TCF_CBQ_LSS_MINIDLE     8
#define TCF_CBQ_LSS_OFFTIME     0x10
#define TCF_CBQ_LSS_AVPKT       0x20
        __u32           maxidle;
        __u32           minidle;
        __u32           offtime;
        __u32           avpkt;
};
#endif

#ifndef TCF_CBQ_LSS_BOUNDED
#define TCF_CBQ_LSS_BOUNDED     1
#define TCF_CBQ_LSS_ISOLATED    2
#endif

#ifndef TC_CBQ_OVL_CLASSIC
struct tc_cbq_wrropt {
        unsigned char   flags;
        unsigned char   priority;
        unsigned char   cpriority;
        unsigned char   __reserved;
        __u32           allot;
        __u32           weight;
};

struct tc_cbq_ovl {
        unsigned char   strategy;
#define TC_CBQ_OVL_CLASSIC      0
#define TC_CBQ_OVL_DELAY        1
#define TC_CBQ_OVL_LOWPRIO      2
#define TC_CBQ_OVL_DROP         3
#define TC_CBQ_OVL_RCLASSIC     4
        unsigned char   priority2;
        __u16           pad;
        __u32           penalty;
};

struct tc_cbq_police {
        unsigned char   police;
        unsigned char   __res1;
        unsigned short  __res2;
};

struct tc_cbq_fopt {
        __u32           split;
        __u32           defmap;
        __u32           defchange;
};

struct tc_cbq_xstats {
        __u32           borrows;
        __u32           overactions;
        __s32           avgidle;
        __s32           undertime;
};
#endif

#ifndef TCA_CBQ_MAX
enum {
        TCA_CBQ_UNSPEC,
        TCA_CBQ_LSSOPT,
        TCA_CBQ_WRROPT,
        TCA_CBQ_FOPT,
        TCA_CBQ_OVL_STRATEGY,
        TCA_CBQ_RATE,
        TCA_CBQ_RTAB,
        TCA_CBQ_POLICE,
        __TCA_CBQ_MAX,
};
#define TCA_CBQ_MAX     (__TCA_CBQ_MAX - 1)
#endif

#endif /* BUSYBOX_CBQ_COMPAT_H */
