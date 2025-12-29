// Author: Lukas Bower
// Purpose: Compile-time profile surface exposing feature-derived constants.

/// Indicates whether the build targets the seL4 kernel environment.
pub const KERNEL: bool = cfg!(feature = "kernel");

/// Indicates whether the PL011 serial console is built in.
pub const SERIAL_CONSOLE: bool = cfg!(feature = "serial-console");

/// Indicates whether the TCP console and networking stack are built in.
pub const NET_CONSOLE: bool = cfg!(feature = "net-console");

/// Indicates whether the base networking feature flag is set.
pub const NET: bool = cfg!(feature = "net");

/// Indicates whether diagnostic networking instrumentation is enabled.
pub const NET_DIAG: bool = cfg!(feature = "net-diag");

/// Consolidated switch mirroring existing net-diag or net-console selections.
pub const NET_DIAG_FEATURED: bool = NET_DIAG || NET_CONSOLE;
