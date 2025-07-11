cmd_coreutils/tail.o := aarch64-linux-gnu-gcc -Wp,-MD,coreutils/.tail.o.d  -std=gnu99 -Iinclude -Ilibbb  -include include/autoconf.h -D_GNU_SOURCE -DNDEBUG -D_LARGEFILE_SOURCE -D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64 -DBB_VER='"1.36.1"' -Wall -Wshadow -Wwrite-strings -Wundef -Wstrict-prototypes -Wunused -Wunused-parameter -Wunused-function -Wunused-value -Wmissing-prototypes -Wmissing-declarations -Wno-format-security -Wdeclaration-after-statement -Wold-style-definition -finline-limit=0 -fno-builtin-strlen -fomit-frame-pointer -ffunction-sections -fdata-sections -fno-guess-branch-probability -funsigned-char -falign-functions=1 -falign-jumps=1 -falign-labels=1 -falign-loops=1 -fno-unwind-tables -fno-asynchronous-unwind-tables -fno-builtin-printf -Oz    -DKBUILD_BASENAME='"tail"'  -DKBUILD_MODNAME='"tail"' -c -o coreutils/tail.o coreutils/tail.c

deps_coreutils/tail.o := \
  coreutils/tail.c \
    $(wildcard include/config/tail.h) \
    $(wildcard include/config/feature/fancy/tail.h) \
    $(wildcard include/config/include/susv2.h) \
    $(wildcard include/config/feature/clean/up.h) \
  /usr/aarch64-linux-gnu/include/stdc-predef.h \
  include/libbb.h \
    $(wildcard include/config/feature/shadowpasswds.h) \
    $(wildcard include/config/use/bb/shadow.h) \
    $(wildcard include/config/selinux.h) \
    $(wildcard include/config/feature/utmp.h) \
    $(wildcard include/config/locale/support.h) \
    $(wildcard include/config/use/bb/pwd/grp.h) \
    $(wildcard include/config/lfs.h) \
    $(wildcard include/config/feature/buffers/go/on/stack.h) \
    $(wildcard include/config/feature/buffers/go/in/bss.h) \
    $(wildcard include/config/extra/cflags.h) \
    $(wildcard include/config/variable/arch/pagesize.h) \
    $(wildcard include/config/feature/verbose.h) \
    $(wildcard include/config/feature/etc/services.h) \
    $(wildcard include/config/feature/ipv6.h) \
    $(wildcard include/config/feature/seamless/xz.h) \
    $(wildcard include/config/feature/seamless/lzma.h) \
    $(wildcard include/config/feature/seamless/bz2.h) \
    $(wildcard include/config/feature/seamless/gz.h) \
    $(wildcard include/config/feature/seamless/z.h) \
    $(wildcard include/config/float/duration.h) \
    $(wildcard include/config/feature/check/names.h) \
    $(wildcard include/config/feature/prefer/applets.h) \
    $(wildcard include/config/long/opts.h) \
    $(wildcard include/config/feature/pidfile.h) \
    $(wildcard include/config/feature/syslog.h) \
    $(wildcard include/config/feature/syslog/info.h) \
    $(wildcard include/config/warn/simple/msg.h) \
    $(wildcard include/config/feature/individual.h) \
    $(wildcard include/config/shell/ash.h) \
    $(wildcard include/config/shell/hush.h) \
    $(wildcard include/config/echo.h) \
    $(wildcard include/config/sleep.h) \
    $(wildcard include/config/printf.h) \
    $(wildcard include/config/test.h) \
    $(wildcard include/config/test1.h) \
    $(wildcard include/config/test2.h) \
    $(wildcard include/config/kill.h) \
    $(wildcard include/config/killall.h) \
    $(wildcard include/config/killall5.h) \
    $(wildcard include/config/chown.h) \
    $(wildcard include/config/ls.h) \
    $(wildcard include/config/xxx.h) \
    $(wildcard include/config/route.h) \
    $(wildcard include/config/feature/hwib.h) \
    $(wildcard include/config/desktop.h) \
    $(wildcard include/config/feature/crond/d.h) \
    $(wildcard include/config/feature/setpriv/capabilities.h) \
    $(wildcard include/config/run/init.h) \
    $(wildcard include/config/feature/securetty.h) \
    $(wildcard include/config/pam.h) \
    $(wildcard include/config/use/bb/crypt.h) \
    $(wildcard include/config/feature/adduser/to/group.h) \
    $(wildcard include/config/feature/del/user/from/group.h) \
    $(wildcard include/config/ioctl/hex2str/error.h) \
    $(wildcard include/config/feature/editing.h) \
    $(wildcard include/config/feature/editing/history.h) \
    $(wildcard include/config/feature/tab/completion.h) \
    $(wildcard include/config/feature/username/completion.h) \
    $(wildcard include/config/feature/editing/fancy/prompt.h) \
    $(wildcard include/config/feature/editing/savehistory.h) \
    $(wildcard include/config/feature/editing/vi.h) \
    $(wildcard include/config/feature/editing/save/on/exit.h) \
    $(wildcard include/config/pmap.h) \
    $(wildcard include/config/feature/show/threads.h) \
    $(wildcard include/config/feature/ps/additional/columns.h) \
    $(wildcard include/config/feature/topmem.h) \
    $(wildcard include/config/feature/top/smp/process.h) \
    $(wildcard include/config/pgrep.h) \
    $(wildcard include/config/pkill.h) \
    $(wildcard include/config/pidof.h) \
    $(wildcard include/config/sestatus.h) \
    $(wildcard include/config/unicode/support.h) \
    $(wildcard include/config/feature/mtab/support.h) \
    $(wildcard include/config/feature/devfs.h) \
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
  /usr/aarch64-linux-gnu/include/ctype.h \
  /usr/aarch64-linux-gnu/include/bits/types/locale_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__locale_t.h \
  /usr/aarch64-linux-gnu/include/dirent.h \
  /usr/aarch64-linux-gnu/include/bits/dirent.h \
  /usr/aarch64-linux-gnu/include/bits/dirent_ext.h \
  /usr/aarch64-linux-gnu/include/errno.h \
  /usr/aarch64-linux-gnu/include/bits/errno.h \
  /usr/aarch64-linux-gnu/include/linux/errno.h \
  /usr/aarch64-linux-gnu/include/asm/errno.h \
  /usr/aarch64-linux-gnu/include/asm-generic/errno.h \
  /usr/aarch64-linux-gnu/include/asm-generic/errno-base.h \
  /usr/aarch64-linux-gnu/include/bits/types/error_t.h \
  /usr/aarch64-linux-gnu/include/fcntl.h \
  /usr/aarch64-linux-gnu/include/bits/fcntl.h \
  /usr/aarch64-linux-gnu/include/bits/fcntl-linux.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_iovec.h \
  /usr/aarch64-linux-gnu/include/linux/falloc.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_timespec.h \
  /usr/aarch64-linux-gnu/include/bits/types/time_t.h \
  /usr/aarch64-linux-gnu/include/bits/stat.h \
  /usr/aarch64-linux-gnu/include/bits/struct_stat.h \
  /usr/aarch64-linux-gnu/include/bits/fcntl2.h \
  /usr/aarch64-linux-gnu/include/inttypes.h \
  /usr/aarch64-linux-gnu/include/netdb.h \
  /usr/aarch64-linux-gnu/include/netinet/in.h \
  /usr/aarch64-linux-gnu/include/sys/socket.h \
  /usr/aarch64-linux-gnu/include/bits/socket.h \
  /usr/aarch64-linux-gnu/include/sys/types.h \
  /usr/aarch64-linux-gnu/include/bits/types/clock_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/clockid_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/timer_t.h \
  /usr/aarch64-linux-gnu/include/sys/select.h \
  /usr/aarch64-linux-gnu/include/bits/select.h \
  /usr/aarch64-linux-gnu/include/bits/types/sigset_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__sigset_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_timeval.h \
  /usr/aarch64-linux-gnu/include/bits/select2.h \
  /usr/aarch64-linux-gnu/include/bits/select-decl.h \
  /usr/aarch64-linux-gnu/include/bits/pthreadtypes.h \
  /usr/aarch64-linux-gnu/include/bits/thread-shared-types.h \
  /usr/aarch64-linux-gnu/include/bits/pthreadtypes-arch.h \
  /usr/aarch64-linux-gnu/include/bits/atomic_wide_counter.h \
  /usr/aarch64-linux-gnu/include/bits/struct_mutex.h \
  /usr/aarch64-linux-gnu/include/bits/struct_rwlock.h \
  /usr/aarch64-linux-gnu/include/bits/socket_type.h \
  /usr/aarch64-linux-gnu/include/bits/sockaddr.h \
  /usr/aarch64-linux-gnu/include/asm/socket.h \
  /usr/aarch64-linux-gnu/include/asm-generic/socket.h \
  /usr/aarch64-linux-gnu/include/linux/posix_types.h \
  /usr/aarch64-linux-gnu/include/linux/stddef.h \
  /usr/aarch64-linux-gnu/include/asm/posix_types.h \
  /usr/aarch64-linux-gnu/include/asm-generic/posix_types.h \
  /usr/aarch64-linux-gnu/include/asm/bitsperlong.h \
  /usr/aarch64-linux-gnu/include/asm-generic/bitsperlong.h \
    $(wildcard include/config/64bit.h) \
  /usr/aarch64-linux-gnu/include/asm/sockios.h \
  /usr/aarch64-linux-gnu/include/asm-generic/sockios.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_osockaddr.h \
  /usr/aarch64-linux-gnu/include/bits/socket2.h \
  /usr/aarch64-linux-gnu/include/bits/in.h \
  /usr/aarch64-linux-gnu/include/rpc/netdb.h \
  /usr/aarch64-linux-gnu/include/bits/types/sigevent_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__sigval_t.h \
  /usr/aarch64-linux-gnu/include/bits/netdb.h \
  /usr/aarch64-linux-gnu/include/setjmp.h \
  /usr/aarch64-linux-gnu/include/bits/setjmp.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct___jmp_buf_tag.h \
  /usr/aarch64-linux-gnu/include/bits/setjmp2.h \
  /usr/aarch64-linux-gnu/include/signal.h \
  /usr/aarch64-linux-gnu/include/bits/signum-generic.h \
  /usr/aarch64-linux-gnu/include/bits/signum-arch.h \
  /usr/aarch64-linux-gnu/include/bits/types/sig_atomic_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/siginfo_t.h \
  /usr/aarch64-linux-gnu/include/bits/siginfo-arch.h \
  /usr/aarch64-linux-gnu/include/bits/siginfo-consts.h \
  /usr/aarch64-linux-gnu/include/bits/siginfo-consts-arch.h \
  /usr/aarch64-linux-gnu/include/bits/types/sigval_t.h \
  /usr/aarch64-linux-gnu/include/bits/sigevent-consts.h \
  /usr/aarch64-linux-gnu/include/bits/sigaction.h \
  /usr/aarch64-linux-gnu/include/bits/sigcontext.h \
  /usr/aarch64-linux-gnu/include/asm/sigcontext.h \
  /usr/aarch64-linux-gnu/include/linux/types.h \
  /usr/aarch64-linux-gnu/include/asm/types.h \
  /usr/aarch64-linux-gnu/include/asm-generic/types.h \
  /usr/aarch64-linux-gnu/include/asm-generic/int-ll64.h \
  /usr/aarch64-linux-gnu/include/asm/sve_context.h \
  /usr/aarch64-linux-gnu/include/bits/types/stack_t.h \
  /usr/aarch64-linux-gnu/include/sys/ucontext.h \
  /usr/aarch64-linux-gnu/include/sys/procfs.h \
  /usr/aarch64-linux-gnu/include/sys/time.h \
  /usr/aarch64-linux-gnu/include/sys/user.h \
  /usr/aarch64-linux-gnu/include/bits/procfs.h \
  /usr/aarch64-linux-gnu/include/bits/procfs-id.h \
  /usr/aarch64-linux-gnu/include/bits/procfs-prregset.h \
  /usr/aarch64-linux-gnu/include/bits/procfs-extra.h \
  /usr/aarch64-linux-gnu/include/bits/sigstack.h \
  /usr/aarch64-linux-gnu/include/bits/sigstksz.h \
  /usr/aarch64-linux-gnu/include/bits/ss_flags.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_sigstack.h \
  /usr/aarch64-linux-gnu/include/bits/sigthread.h \
  /usr/aarch64-linux-gnu/include/bits/signal_ext.h \
  /usr/aarch64-linux-gnu/include/paths.h \
  /usr/aarch64-linux-gnu/include/stdio.h \
  /usr/lib/gcc/aarch64-linux-gnu/13/include/stdarg.h \
  /usr/aarch64-linux-gnu/include/bits/types/__fpos_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__mbstate_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__fpos64_t.h \
  /usr/aarch64-linux-gnu/include/bits/types/__FILE.h \
  /usr/aarch64-linux-gnu/include/bits/types/FILE.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_FILE.h \
  /usr/aarch64-linux-gnu/include/bits/types/cookie_io_functions_t.h \
  /usr/aarch64-linux-gnu/include/bits/stdio_lim.h \
  /usr/aarch64-linux-gnu/include/bits/floatn.h \
  /usr/aarch64-linux-gnu/include/bits/floatn-common.h \
  /usr/aarch64-linux-gnu/include/bits/stdio2-decl.h \
  /usr/aarch64-linux-gnu/include/bits/stdio2.h \
  /usr/aarch64-linux-gnu/include/stdlib.h \
  /usr/aarch64-linux-gnu/include/bits/waitflags.h \
  /usr/aarch64-linux-gnu/include/bits/waitstatus.h \
  /usr/aarch64-linux-gnu/include/alloca.h \
  /usr/aarch64-linux-gnu/include/bits/stdlib-float.h \
  /usr/aarch64-linux-gnu/include/bits/stdlib.h \
  /usr/aarch64-linux-gnu/include/string.h \
  /usr/aarch64-linux-gnu/include/strings.h \
  /usr/aarch64-linux-gnu/include/bits/strings_fortified.h \
  /usr/aarch64-linux-gnu/include/bits/string_fortified.h \
  /usr/aarch64-linux-gnu/include/libgen.h \
  /usr/aarch64-linux-gnu/include/poll.h \
  /usr/aarch64-linux-gnu/include/sys/poll.h \
  /usr/aarch64-linux-gnu/include/bits/poll.h \
  /usr/aarch64-linux-gnu/include/bits/poll2.h \
  /usr/aarch64-linux-gnu/include/sys/ioctl.h \
  /usr/aarch64-linux-gnu/include/bits/ioctls.h \
  /usr/aarch64-linux-gnu/include/asm/ioctls.h \
  /usr/aarch64-linux-gnu/include/asm-generic/ioctls.h \
  /usr/aarch64-linux-gnu/include/linux/ioctl.h \
  /usr/aarch64-linux-gnu/include/asm/ioctl.h \
  /usr/aarch64-linux-gnu/include/asm-generic/ioctl.h \
  /usr/aarch64-linux-gnu/include/bits/ioctl-types.h \
  /usr/aarch64-linux-gnu/include/sys/ttydefaults.h \
  /usr/aarch64-linux-gnu/include/sys/mman.h \
  /usr/aarch64-linux-gnu/include/bits/mman.h \
  /usr/aarch64-linux-gnu/include/bits/mman-map-flags-generic.h \
  /usr/aarch64-linux-gnu/include/bits/mman-linux.h \
  /usr/aarch64-linux-gnu/include/bits/mman-shared.h \
  /usr/aarch64-linux-gnu/include/bits/mman_ext.h \
  /usr/aarch64-linux-gnu/include/sys/resource.h \
  /usr/aarch64-linux-gnu/include/bits/resource.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_rusage.h \
  /usr/aarch64-linux-gnu/include/sys/stat.h \
  /usr/aarch64-linux-gnu/include/bits/statx.h \
  /usr/aarch64-linux-gnu/include/linux/stat.h \
  /usr/aarch64-linux-gnu/include/bits/statx-generic.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_statx_timestamp.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_statx.h \
  /usr/aarch64-linux-gnu/include/sys/sysmacros.h \
  /usr/aarch64-linux-gnu/include/bits/sysmacros.h \
  /usr/aarch64-linux-gnu/include/sys/wait.h \
  /usr/aarch64-linux-gnu/include/bits/types/idtype_t.h \
  /usr/aarch64-linux-gnu/include/termios.h \
  /usr/aarch64-linux-gnu/include/bits/termios.h \
  /usr/aarch64-linux-gnu/include/bits/termios-struct.h \
  /usr/aarch64-linux-gnu/include/bits/termios-c_cc.h \
  /usr/aarch64-linux-gnu/include/bits/termios-c_iflag.h \
  /usr/aarch64-linux-gnu/include/bits/termios-c_oflag.h \
  /usr/aarch64-linux-gnu/include/bits/termios-baud.h \
  /usr/aarch64-linux-gnu/include/bits/termios-c_cflag.h \
  /usr/aarch64-linux-gnu/include/bits/termios-c_lflag.h \
  /usr/aarch64-linux-gnu/include/bits/termios-tcflow.h \
  /usr/aarch64-linux-gnu/include/bits/termios-misc.h \
  /usr/aarch64-linux-gnu/include/time.h \
  /usr/aarch64-linux-gnu/include/bits/time.h \
  /usr/aarch64-linux-gnu/include/bits/timex.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_tm.h \
  /usr/aarch64-linux-gnu/include/bits/types/struct_itimerspec.h \
  /usr/aarch64-linux-gnu/include/sys/param.h \
  /usr/aarch64-linux-gnu/include/bits/param.h \
  /usr/aarch64-linux-gnu/include/linux/param.h \
  /usr/aarch64-linux-gnu/include/asm/param.h \
  /usr/aarch64-linux-gnu/include/asm-generic/param.h \
  /usr/aarch64-linux-gnu/include/pwd.h \
  /usr/aarch64-linux-gnu/include/grp.h \
  /usr/aarch64-linux-gnu/include/mntent.h \
  /usr/aarch64-linux-gnu/include/sys/statfs.h \
  /usr/aarch64-linux-gnu/include/bits/statfs.h \
  /usr/aarch64-linux-gnu/include/arpa/inet.h \
  include/xatonum.h \
  include/common_bufsiz.h \

coreutils/tail.o: $(deps_coreutils/tail.o)

$(deps_coreutils/tail.o):
