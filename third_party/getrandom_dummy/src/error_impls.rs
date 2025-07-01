// CLASSIFICATION: COMMUNITY
// Filename: error_impls.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

extern crate std;

use crate::Error;
use std::io;

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        match err.raw_os_error() {
            Some(errno) => {
                // Cast errno to the RawOsError type expected by from_raw_os_error.
                // UEFI defines RawOsError as `usize`, while most targets expect `i32`.
                #[cfg(target_os = "uefi")]
                {
                    io::Error::from_raw_os_error(errno as usize)
                }
                #[cfg(not(target_os = "uefi"))]
                {
                    io::Error::from_raw_os_error(errno as i32)
                }
            }
            None => io::Error::new(io::ErrorKind::Other, err),
        }
    }
}

impl std::error::Error for Error {}
