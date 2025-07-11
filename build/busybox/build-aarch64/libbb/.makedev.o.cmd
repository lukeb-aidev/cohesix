cmd_libbb/makedev.o := aarch64-linux-gnu-gcc -Wp,-MD,libbb/.makedev.o.d  -std=gnu99 -Iinclude -Ilibbb  -include include/autoconf.h -D_GNU_SOURCE -DNDEBUG -D_LARGEFILE_SOURCE -D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64 -DBB_VER='"1.36.1"' -Wall -Wshadow -Wwrite-strings -Wundef -Wstrict-prototypes -Wunused -Wunused-parameter -Wunused-function -Wunused-value -Wmissing-prototypes -Wmissing-declarations -Wno-format-security -Wdeclaration-after-statement -Wold-style-definition -finline-limit=0 -fno-builtin-strlen -fomit-frame-pointer -ffunction-sections -fdata-sections -fno-guess-branch-probability -funsigned-char -falign-functions=1 -falign-jumps=1 -falign-labels=1 -falign-loops=1 -fno-unwind-tables -fno-asynchronous-unwind-tables -fno-builtin-printf -Oz    -DKBUILD_BASENAME='"makedev"'  -DKBUILD_MODNAME='"makedev"' -c -o libbb/makedev.o libbb/makedev.c

deps_libbb/makedev.o := \
  libbb/makedev.c \
  /usr/aarch64-linux-gnu/include/stdc-predef.h \
  include/platform.h \
    $(wildcard include/config/werror.h) \
    $(wildcard include/config/big/endian.h) \
    $(wildcard include/config/little/endian.h) \
    $(wildcard include/config/nommu.h) \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/limits.h \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/syslimits.h \
  /usr/aarch64-linux-gnu/include/limits.h \
  /usr/aarch64-linux-gnu/include/bits/libc-header-start.h \
  /usr/aarch64-linux-gnu/include/features.h \
  /usr/aarch64-linux-gnu/include/features-time64.h \
  /usr/aarch64-linux-gnu/include/bits/wordsize.h \
  /usr/aarch64-linux-gnu/include/bits/timesize.h \
  /usr/aarch64-linux-gnu/include/sys/cdefs.h \
  /usr/aarch64-linux-gnu/include/bits/long-double.h \
  /usr/aarch64-linux-gnu/include/gnu/stubs.h \
  /usr/aarch64-linux-gnu/include/gnu/stubs-lp64.h \
  /usr/aarch64-linux-gnu/include/bits/posix1_lim.h \
  /usr/aarch64-linux-gnu/include/bits/local_lim.h \
  /usr/aarch64-linux-gnu/include/linux/limits.h \
  /usr/aarch64-linux-gnu/include/bits/pthread_stack_min-dynamic.h \
  /usr/aarch64-linux-gnu/include/bits/posix2_lim.h \
  /usr/aarch64-linux-gnu/include/bits/xopen_lim.h \
  /usr/aarch64-linux-gnu/include/bits/uio_lim.h \
  /usr/aarch64-linux-gnu/include/byteswap.h \
  /usr/aarch64-linux-gnu/include/bits/byteswap.h \
  /usr/aarch64-linux-gnu/include/bits/types.h \
  /usr/aarch64-linux-gnu/include/bits/typesizes.h \
  /usr/aarch64-linux-gnu/include/bits/time64.h \
  /usr/aarch64-linux-gnu/include/endian.h \
  /usr/aarch64-linux-gnu/include/bits/endian.h \
  /usr/aarch64-linux-gnu/include/bits/endianness.h \
  /usr/aarch64-linux-gnu/include/bits/uintn-identity.h \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/stdint.h \
  /usr/aarch64-linux-gnu/include/stdint.h \
  /usr/aarch64-linux-gnu/include/bits/wchar.h \
  /usr/aarch64-linux-gnu/include/bits/stdint-intn.h \
  /usr/aarch64-linux-gnu/include/bits/stdint-uintn.h \
  /usr/aarch64-linux-gnu/include/bits/stdint-least.h \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/stdbool.h \
  /usr/aarch64-linux-gnu/include/unistd.h \
  /usr/aarch64-linux-gnu/include/bits/posix_opt.h \
  /usr/aarch64-linux-gnu/include/bits/environments.h \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/stddef.h \
  /usr/aarch64-linux-gnu/include/bits/confname.h \
  /usr/aarch64-linux-gnu/include/bits/getopt_posix.h \
  /usr/aarch64-linux-gnu/include/bits/getopt_core.h \
  /usr/aarch64-linux-gnu/include/bits/unistd.h \
  /usr/aarch64-linux-gnu/include/bits/unistd-decl.h \
  /usr/aarch64-linux-gnu/include/bits/unistd_ext.h \
  /usr/aarch64-linux-gnu/include/linux/close_range.h \
  /usr/aarch64-linux-gnu/include/sys/sysmacros.h \
  /usr/aarch64-linux-gnu/include/bits/sysmacros.h \

libbb/makedev.o: $(deps_libbb/makedev.o)

$(deps_libbb/makedev.o):
