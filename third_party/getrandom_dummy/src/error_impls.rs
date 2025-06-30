// CLASSIFICATION: COMMUNITY
// Filename: error_impls.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-25

extern crate std;

use crate::Error;
use std::io;

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        match err.raw_os_error() {
            // Cast errno to the RawOsError type expected by from_raw_os_error.
            // UEFI defines RawOsError as `usize`, so this conversion is safe.
            Some(errno) => io::Error::from_raw_os_error(errno as usize),
            None => io::Error::new(io::ErrorKind::Other, err),
        }
    }
}

impl std::error::Error for Error {}
