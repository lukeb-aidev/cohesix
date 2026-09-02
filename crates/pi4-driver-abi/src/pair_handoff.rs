// Author: Lukas Bower
// Purpose: Retain passive first-child CYW43/SDIO handoff evidence without granting work.
// Copyright 2026 Lukas Bower

/// Unused ring bytes after the 20-byte completion at 64 and before progress at 128.
pub const DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET: usize = 84;
/// The record does not overlap command, completion, grants, progress, or DPC state.
pub const DRIVER_RUNTIME_PAIR_HANDOFF_WORDS: usize = 11;
/// Passive record discriminator (`PHOF`), independent of runtime-init ABI v13.
pub const DRIVER_RUNTIME_PAIR_HANDOFF_MAGIC: u32 = 0x5048_4f46;
/// Passive diagnostic layout version; no field is admission authority.
pub const DRIVER_RUNTIME_PAIR_HANDOFF_VERSION: u16 = 1;
/// The sole writer is the local SDIO runtime.
pub const PAIR_HANDOFF_SDIO: u8 = 1;
/// The sole writer is the local CYW43 runtime, never its peer's record.
pub const PAIR_HANDOFF_CYW43: u8 = 2;

/// The owner completed engine init, or the producer prepared its first child.
pub const PAIR_HANDOFF_ARMED: u32 = 1 << 0;
/// The owner reached its actual blocking receive call site.
pub const PAIR_HANDOFF_PREWAIT: u32 = 1 << 1;
/// A syscall returned a raw receive result, before badge/IPC classification.
pub const PAIR_HANDOFF_RAW_WAKE: u32 = 1 << 2;
/// The owner stable-read the selected nonzero child command.
pub const PAIR_HANDOFF_RING_SEEN: u32 = 1 << 3;
/// The command reached intake before descriptor sealing.
pub const PAIR_HANDOFF_INTAKE_BEGIN: u32 = 1 << 4;
/// Descriptor pre-admission returned true (not physical issue permission).
pub const PAIR_HANDOFF_SEALED: u32 = 1 << 5;
/// The exact child entered its normal typed dispatcher.
pub const PAIR_HANDOFF_DISPATCH: u32 = 1 << 6;
/// One normal bounded service quantum returned; not necessarily physical I/O.
pub const PAIR_HANDOFF_ACTION_RETURNED: u32 = 1 << 7;
/// The exact child's normal terminal was committed.
pub const PAIR_HANDOFF_TERMINAL: u32 = 1 << 8;
/// Producer route selection completed before the command's sequence-last commit.
pub const PAIR_HANDOFF_PRECOMMIT: u32 = 1 << 9;
/// The selected producer signal/atomic-wait syscall returned.
pub const PAIR_HANDOFF_SEND_RETURNED: u32 = 1 << 10;
/// The producer entered its existing pair-recovery path.
pub const PAIR_HANDOFF_RECOVERY: u32 = 1 << 11;
/// The producer observed an exact child terminal or first-action receipt.
pub const PAIR_HANDOFF_CHILD_RETURNED: u32 = 1 << 12;
/// A passive route/intake observation found a rejection; it does not cause one.
pub const PAIR_HANDOFF_REJECTED: u32 = 1 << 13;
/// Complete known stage mask.
pub const PAIR_HANDOFF_STAGE_MASK: u32 = (1 << 14) - 1;

/// One bounded, first-child-only record in each runtime's existing local ring.
///
/// `publication` changes on every retained stage, and is committed last. The
/// consumer must double-read an equal valid record. The record freezes at the
/// first child's terminal/recovery and is cleared only with canonical runtime
/// reset. Missing/torn diagnostics must never alter driver decisions.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverRuntimePairHandoff {
    /// Fixed magic.
    pub magic: u32,
    /// Passive layout version.
    pub version: u16,
    /// SDIO or CYW43 sole-writer role.
    pub role: u8,
    /// Route: 0 unavailable, 1 SDIO, 2 CYW43, 3 other; high nibble is wait kind.
    pub route: u8,
    /// Nonzero diagnostic publication revision, not a command or grant sequence.
    pub publication: u32,
    /// First child sequence; zero only before owner intake.
    pub request: u32,
    /// CYW43 parent sequence, or SDIO's retired engine sequence.
    pub parent: u32,
    /// Exact child command `aux1`; zero until the owner sees a command.
    pub generation: u32,
    /// Monotonic stage bits, not a work budget or progress grant.
    pub stages: u32,
    /// First raw owner badge, or producer precommit rejection phase (zero if admitted).
    pub detail: u32,
    /// Raw owner MessageInfo low word, or producer's first returned wait badge.
    pub witness: u32,
    /// CNTVCT low word at the most recently retained stage; zero means unavailable.
    pub cntvct_lo: u32,
    /// Commit-last repetition of the publication revision.
    pub committed_publication: u32,
}

impl DriverRuntimePairHandoff {
    /// Inactive record, never accepted as evidence.
    pub const fn empty() -> Self {
        Self {
            magic: DRIVER_RUNTIME_PAIR_HANDOFF_MAGIC,
            version: DRIVER_RUNTIME_PAIR_HANDOFF_VERSION,
            role: 0,
            route: 0,
            publication: 0,
            request: 0,
            parent: 0,
            generation: 0,
            stages: 0,
            detail: 0,
            witness: 0,
            cntvct_lo: 0,
            committed_publication: 0,
        }
    }

    /// Decode primitive words without transmute, pointer casts, or unsafe code.
    pub const fn from_words(words: [u32; DRIVER_RUNTIME_PAIR_HANDOFF_WORDS]) -> Self {
        Self {
            magic: words[0],
            version: words[1] as u16,
            role: (words[1] >> 16) as u8,
            route: (words[1] >> 24) as u8,
            publication: words[2],
            request: words[3],
            parent: words[4],
            generation: words[5],
            stages: words[6],
            detail: words[7],
            witness: words[8],
            cntvct_lo: words[9],
            committed_publication: words[10],
        }
    }

    /// Encode the fixed little-word contract; memory accessors own byte order.
    pub const fn words(self) -> [u32; DRIVER_RUNTIME_PAIR_HANDOFF_WORDS] {
        [
            self.magic,
            self.version as u32 | ((self.role as u32) << 16) | ((self.route as u32) << 24),
            self.publication,
            self.request,
            self.parent,
            self.generation,
            self.stages,
            self.detail,
            self.witness,
            self.cntvct_lo,
            self.committed_publication,
        ]
    }

    /// Reject unknown layouts/roles/stages and uncommitted revisions.
    pub const fn valid(self) -> bool {
        self.magic == DRIVER_RUNTIME_PAIR_HANDOFF_MAGIC
            && self.version == DRIVER_RUNTIME_PAIR_HANDOFF_VERSION
            && matches!(self.role, PAIR_HANDOFF_SDIO | PAIR_HANDOFF_CYW43)
            && self.route & 0x0f <= 3
            && self.route >> 4 <= 4
            && self.publication != 0
            && self.publication == self.committed_publication
            && self.stages & PAIR_HANDOFF_ARMED != 0
            && self.stages & !PAIR_HANDOFF_STAGE_MASK == 0
            && (self.request != 0 || self.role == PAIR_HANDOFF_SDIO)
    }
}

const _: () = {
    assert!(core::mem::size_of::<DriverRuntimePairHandoff>() == 44);
    assert!(core::mem::offset_of!(DriverRuntimePairHandoff, committed_publication) == 40);
    assert!(
        DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET + 44
            == super::DRIVER_RUNTIME_RING_PROGRESS_OFFSET as usize
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_handoff_layout_and_commit_are_independent_of_command_authority() {
        let words = [
            0x5048_4f46,
            0x2101_0001,
            7,
            0x8000_0011,
            2,
            9,
            0x19,
            256,
            1,
            54,
            7,
        ];
        let record = DriverRuntimePairHandoff::from_words(words);
        assert!(record.valid());
        assert_eq!(record.words(), words);
        assert_eq!(record.role, 1);
        assert_eq!(record.route, 0x21);
        assert_eq!(DRIVER_RUNTIME_PAIR_HANDOFF_OFFSET, 84);
        assert_eq!(core::mem::size_of::<DriverRuntimePairHandoff>(), 44);
        assert!(!DriverRuntimePairHandoff::empty().valid());
        for index in [0, 1, 2, 10] {
            let mut torn = words;
            torn[index] ^= 1;
            assert!(!DriverRuntimePairHandoff::from_words(torn).valid());
        }
        let mut unknown = record;
        unknown.stages |= 1 << 31;
        assert!(!unknown.valid());
    }
}
