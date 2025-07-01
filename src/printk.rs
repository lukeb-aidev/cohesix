// CLASSIFICATION: COMMUNITY
// Filename: printk.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

// Minimal logging macros for no_std UEFI builds.
// Provides stand-ins for println!, eprintln!, print!, and dbg!.

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        log::info!($($arg)*);
    };
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {
        log::error!($($arg)*);
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        log::info!($($arg)*);
    };
}

#[macro_export]
macro_rules! dbg {
    () => {
        log::debug!("[dbg]");
    };
    ($val:expr $(,)?) => {{
        let tmp = &$val;
        log::debug!(concat!(stringify!($val), " = {:?}"), tmp);
        tmp
    }};
    ($($val:expr),+ $(,)?) => {{
        ($(dbg!($val)),+,)
    }};
}
