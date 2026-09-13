// Author: Lukas Bower
// Purpose: Parse the bounded TPM 2.0 quote and ECDSA signature wire structures.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use crate::{Error, TrustPolicy};
use alloc::vec::Vec;

pub(crate) struct Quote<'a> {
    pub signer: &'a [u8],
    pub extra_data: &'a [u8],
    pub clock: u64,
    pub reset_count: u32,
    pub restart_count: u32,
    pub pcr_digest: &'a [u8],
}

impl<'a> Quote<'a> {
    pub fn parse(bytes: &'a [u8], policy: &TrustPolicy) -> Result<Self, Error> {
        let mut input = Reader(bytes);
        // TCG TPM Library Part 2: TPM_GENERATED_VALUE, TPM_ST_ATTEST_QUOTE.
        if input.u32()? != 0xff54_4347 || input.u16()? != 0x8018 {
            return Err(Error::Schema);
        }
        let signer = input.sized(34)?;
        let extra_data = input.sized(64)?;
        let clock = input.u64()?;
        let reset_count = input.u32()?;
        let restart_count = input.u32()?;
        if input.take(1)? != [1] || clock < policy.minimum_clock {
            return Err(Error::Clock);
        }
        if reset_count != policy.reset_count || restart_count != policy.restart_count {
            return Err(Error::TpmReset);
        }
        let _firmware_version = input.u64()?;
        // A single SHA-256 bank with the 24-PCR bitmap; no implicit selection.
        if input.u32()? != 1 || input.u16()? != 0x000b || input.take(1)? != [3] {
            return Err(Error::PcrSelection);
        }
        let selection = input.take(3)?;
        let mut expected = [0u8; 3];
        for pcr in &policy.pcrs {
            let byte = expected
                .get_mut(usize::from(pcr.index / 8))
                .ok_or(Error::PcrSelection)?;
            *byte |= 1 << (pcr.index % 8);
        }
        if selection != expected {
            return Err(Error::PcrSelection);
        }
        let pcr_digest = input.sized(32)?;
        input.finish()?;
        Ok(Self {
            signer,
            extra_data,
            clock,
            reset_count,
            restart_count,
            pcr_digest,
        })
    }
}

/// Convert TPM's unsigned big-endian R/S scalars to canonical ASN.1 ECDSA.
pub(crate) fn signature_der(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut input = Reader(bytes);
    if input.u16()? != 0x0018 || input.u16()? != 0x000b {
        return Err(Error::UnsupportedAlgorithm);
    }
    let r = input.sized(32)?;
    let s = input.sized(32)?;
    input.finish()?;
    let mut body = Vec::with_capacity(70);
    for scalar in [r, s] {
        let first = scalar
            .iter()
            .position(|byte| *byte != 0)
            .ok_or(Error::Signature)?;
        let scalar = &scalar[first..];
        let padding = usize::from(scalar[0] & 0x80 != 0);
        body.push(2);
        body.push(u8::try_from(scalar.len() + padding).map_err(|_| Error::Bounds)?);
        if padding != 0 {
            body.push(0);
        }
        body.extend_from_slice(scalar);
    }
    let mut out = Vec::with_capacity(72);
    out.push(0x30);
    out.push(u8::try_from(body.len()).map_err(|_| Error::Bounds)?);
    out.extend_from_slice(&body);
    Ok(out)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let value = self.0.get(..count).ok_or(Error::Truncated)?;
        self.0 = &self.0[count..];
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| Error::Truncated)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| Error::Truncated)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| Error::Truncated)?,
        ))
    }
    fn sized(&mut self, maximum: usize) -> Result<&'a [u8], Error> {
        let count = usize::from(self.u16()?);
        if count > maximum {
            return Err(Error::Bounds);
        }
        self.take(count)
    }
    fn finish(self) -> Result<(), Error> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::TrailingBytes)
        }
    }
}
