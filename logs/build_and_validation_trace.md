// CLASSIFICATION: COMMUNITY
// Filename: build_and_validation_trace.md v1.0
// Author: Lukas Bower
// Date Modified: 2027-12-30

# Cohesix AArch64 Build and Validation Trace

## Setup

```
$ sudo apt-get update
$ sudo apt-get install -y repo
```
Output excerpt:
```
Fetched 31.3 MB in 3s (9970 kB/s)
Setting up repo (2.36.1-1) ...
```

## Fetch seL4 Sources

```
$ bash third_party/seL4/fetch_sel4.sh
```
Result:
```
fatal: Cannot get https://gerrit.googlesource.com/git-repo/clone.bundle
fatal: error Tunnel connection failed: 403 Forbidden
fatal: cloning the git-repo repository failed
```

## Build cohesix_root

```
$ cd workspace
$ cargo +nightly build -p cohesix_root --release --target=cohesix_root/sel4-aarch64.json -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem
```
Failure:
```
ld.lld: error: unable to find library -lsel4
error: could not compile `cohesix_root` due to previous error
```

## Result

The build failed because `libsel4.a` was missing, so no `cohesix_root.elf` was produced. Subsequent ELF inspection and QEMU boot validation were not possible.

