cmd_libbb/ptr_to_globals.o := aarch64-linux-gnu-gcc -Wp,-MD,libbb/.ptr_to_globals.o.d  -std=gnu99 -Iinclude -Ilibbb  -include include/autoconf.h -D_GNU_SOURCE -DNDEBUG -D_LARGEFILE_SOURCE -D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64 -DBB_VER='"1.36.1"' -Wall -Wshadow -Wwrite-strings -Wundef -Wstrict-prototypes -Wunused -Wunused-parameter -Wunused-function -Wunused-value -Wmissing-prototypes -Wmissing-declarations -Wno-format-security -Wdeclaration-after-statement -Wold-style-definition -finline-limit=0 -fno-builtin-strlen -fomit-frame-pointer -ffunction-sections -fdata-sections -fno-guess-branch-probability -funsigned-char -falign-functions=1 -falign-jumps=1 -falign-labels=1 -falign-loops=1 -fno-unwind-tables -fno-asynchronous-unwind-tables -fno-builtin-printf -Oz    -DKBUILD_BASENAME='"ptr_to_globals"'  -DKBUILD_MODNAME='"ptr_to_globals"' -c -o libbb/ptr_to_globals.o libbb/ptr_to_globals.c

deps_libbb/ptr_to_globals.o := \
  libbb/ptr_to_globals.c \
  /usr/aarch64-linux-gnu/include/stdc-predef.h \
  /usr/aarch64-linux-gnu/include/errno.h \
  /usr/aarch64-linux-gnu/include/features.h \
  /usr/aarch64-linux-gnu/include/features-time64.h \
  /usr/aarch64-linux-gnu/include/bits/wordsize.h \
  /usr/aarch64-linux-gnu/include/bits/timesize.h \
  /usr/aarch64-linux-gnu/include/sys/cdefs.h \
  /usr/aarch64-linux-gnu/include/bits/long-double.h \
  /usr/aarch64-linux-gnu/include/gnu/stubs.h \
  /usr/aarch64-linux-gnu/include/gnu/stubs-lp64.h \
  /usr/aarch64-linux-gnu/include/bits/errno.h \
  /usr/aarch64-linux-gnu/include/linux/errno.h \
  /usr/aarch64-linux-gnu/include/asm/errno.h \
  /usr/aarch64-linux-gnu/include/asm-generic/errno.h \
  /usr/aarch64-linux-gnu/include/asm-generic/errno-base.h \
  /usr/aarch64-linux-gnu/include/bits/types/error_t.h \

libbb/ptr_to_globals.o: $(deps_libbb/ptr_to_globals.o)

$(deps_libbb/ptr_to_globals.o):
