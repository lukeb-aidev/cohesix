// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Retain owner-sampled kernel CPU accounting across one physical TCP session.
// Author: Lukas Bower

use core::fmt::Write;
use core::sync::atomic::{AtomicU64, Ordering};

use heapless::String;
use spin::Mutex;

use crate::serial::DEFAULT_LINE_CAPACITY;

const ROLES: usize = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Role {
    Root = 0,
    Console = 1,
    Genet = 2,
    Cyw43 = 3,
    Sdio = 4,
    Serial = 5,
    Usb = 6,
    Hdmi = 7,
    Pcie = 8,
}

impl Role {
    const fn label(self) -> &'static str {
        match self {
            Self::Root => "root-control",
            Self::Console => "console-network",
            Self::Genet => "driver-genet",
            Self::Cyw43 => "driver-cyw43",
            Self::Sdio => "driver-sdio",
            Self::Serial => "driver-serial",
            Self::Usb => "driver-usb",
            Self::Hdmi => "driver-hdmi",
            Self::Pcie => "driver-pcie",
        }
    }

    const fn bit(self) -> u16 {
        1 << self as usize
    }
}

const ALL_ROLES: [Role; ROLES] = [
    Role::Root,
    Role::Console,
    Role::Genet,
    Role::Cyw43,
    Role::Sdio,
    Role::Serial,
    Role::Usb,
    Role::Hdmi,
    Role::Pcie,
];

/// Cumulative software receipt of every Consumed drain for this owner. Existing
/// passive-admission drains keep their exact return values and order; recording
/// them prevents an intervening drain from disappearing from the session sum.
static CONSUMED: [AtomicU64; ROLES] = [const { AtomicU64::new(0) }; ROLES];
static ERRORS: [AtomicU64; ROLES] = [const { AtomicU64::new(0) }; ROLES];

pub(crate) fn record_drain(role: Role, consumed_us: Option<u64>) {
    let index = role as usize;
    match consumed_us {
        Some(value) => {
            let old = CONSUMED[index].fetch_add(value, Ordering::Relaxed);
            if old.checked_add(value).is_none() {
                ERRORS[index].fetch_add(1, Ordering::Relaxed);
            }
        }
        None => {
            ERRORS[index].fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Counter captures bracket the actual syscall, so a remote-core stall in the
/// measurement itself remains visible. CPU totals are kernel microseconds;
/// counter ticks are wall time and never stand in for consumed CPU.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Sample {
    pub generation: u64,
    pub entered: u64,
    pub returned: u64,
    pub total_us: u64,
    pub errors: u64,
    pub valid: bool,
}

impl Sample {
    pub(crate) fn capture(
        role: Role,
        generation: u64,
        entered: u64,
        returned: u64,
        valid: bool,
    ) -> Self {
        Self {
            generation,
            entered,
            returned,
            total_us: CONSUMED[role as usize].load(Ordering::Relaxed),
            errors: ERRORS[role as usize].load(Ordering::Relaxed),
            valid: valid && generation != 0 && entered != 0 && returned >= entered,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    epoch: u64,
    finish: bool,
}

#[derive(Clone, Copy)]
struct Session {
    epoch: u64,
    generation: u64,
    connection: u64,
    finish: bool,
    selected: u16,
    pending: u16,
    claimed: u16,
    begin: [Option<Sample>; ROLES],
    end: [Option<Sample>; ROLES],
}

impl Session {
    const fn new() -> Self {
        Self {
            epoch: 0,
            generation: 0,
            connection: 0,
            finish: false,
            selected: 0,
            pending: 0,
            claimed: 0,
            begin: [None; ROLES],
            end: [None; ROLES],
        }
    }

    fn request(
        &mut self,
        generation: u64,
        connection: u64,
        genet: bool,
        finish: bool,
    ) -> Option<Request> {
        if generation == 0 || connection == 0 {
            return None;
        }
        if finish {
            if self.finish || (self.generation, self.connection) != (generation, connection) {
                return None;
            }
            self.finish = true;
        } else {
            if (self.generation, self.connection) == (generation, connection) {
                return None;
            }
            let epoch = self.epoch.checked_add(1)?;
            *self = Self::new();
            self.epoch = epoch;
            self.generation = generation;
            self.connection = connection;
            self.selected = Role::Root.bit()
                | Role::Console.bit()
                | Role::Serial.bit()
                | Role::Usb.bit()
                | Role::Hdmi.bit()
                | Role::Pcie.bit()
                | if genet {
                    Role::Genet.bit()
                } else {
                    Role::Cyw43.bit() | Role::Sdio.bit()
                };
        }
        self.pending = self.selected;
        self.claimed = 0;
        Some(Request {
            epoch: self.epoch,
            finish,
        })
    }

    fn store(&mut self, request: Request, role: Role, sample: Sample) {
        if request.epoch != self.epoch
            || request.finish != self.finish
            || (self.pending | self.claimed) & role.bit() == 0
        {
            return;
        }
        if request.finish {
            self.end[role as usize] = Some(sample);
        } else {
            self.begin[role as usize] = Some(sample);
        }
        self.pending &= !role.bit();
        self.claimed &= !role.bit();
    }

    fn delta(&self, role: Role) -> Option<u64> {
        let begin = self.begin[role as usize]?;
        let end = self.end[role as usize]?;
        if !begin.valid
            || !end.valid
            || begin.generation != end.generation
            || begin.errors != end.errors
            || end.entered < begin.returned
        {
            return None;
        }
        end.total_us.checked_sub(begin.total_us)
    }

    fn claim_driver(&mut self) -> Option<(Request, Role)> {
        let role = [
            Role::Genet,
            Role::Cyw43,
            Role::Sdio,
            Role::Serial,
            Role::Usb,
            Role::Hdmi,
            Role::Pcie,
        ]
        .into_iter()
        .find(|role| self.pending & role.bit() != 0)?;
        self.pending &= !role.bit();
        self.claimed |= role.bit();
        Some((
            Request {
                epoch: self.epoch,
                finish: self.finish,
            },
            role,
        ))
    }
}

static SESSION: Mutex<Session> = Mutex::new(Session::new());

pub(crate) fn request(
    generation: u64,
    connection: u64,
    genet: bool,
    finish: bool,
) -> Option<Request> {
    SESSION
        .try_lock()?
        .request(generation, connection, genet, finish)
}

pub(crate) fn store(request: Request, role: Role, sample: Sample) {
    if let Some(mut session) = SESSION.try_lock() {
        session.store(request, role, sample);
    }
}

/// The supervisor admits at most one selected physical-driver read per wake.
/// A finite remainder reuses its existing self-signal; no timer or polling lane
/// is introduced. Missing, stale, or raced samples remain visibly incomplete.
pub(crate) fn driver_request() -> Option<(Request, Role)> {
    SESSION.try_lock()?.claim_driver()
}

pub(crate) fn driver_pending() -> bool {
    SESSION
        .try_lock()
        .is_some_and(|session| session.pending & 0b111111100 != 0)
}

pub(crate) fn lines() -> [String<DEFAULT_LINE_CAPACITY>; ROLES + 1] {
    let mut lines = core::array::from_fn(|_| String::new());
    let Some(session) = SESSION.try_lock().map(|value| *value) else {
        let _ = lines[0].push_str("[smp:consumed/v1] state=contended");
        return lines;
    };
    render(
        &session,
        crate::generated::console_network_service_config().timer_clock_hz,
    )
}

fn render(session: &Session, timer_hz: u64) -> [String<DEFAULT_LINE_CAPACITY>; ROLES + 1] {
    let mut lines = core::array::from_fn(|_| String::new());
    let _ = write!(lines[0], "[smp:consumed/v1] generation={} conn={} ended={} selected={:x} pending={:x} claimed={:x} hz={}",
        session.generation, session.connection, session.finish, session.selected, session.pending, session.claimed,
        timer_hz);
    for (index, role) in ALL_ROLES.into_iter().enumerate() {
        if session.selected & role.bit() == 0 {
            continue;
        }
        let begin = session.begin[index].unwrap_or_default();
        let end = session.end[index].unwrap_or_default();
        let delta = session.delta(role);
        let _ = write!(lines[index + 1],
            "[smp:consumed/v1] task={} valid={} cpu_us={} cap_gen={:x}/{:x} begin={:x}/{:x} end={:x}/{:x}",
            role.label(), delta.is_some(), delta.unwrap_or(0), begin.generation, end.generation,
            begin.entered, begin.returned, end.entered, end.returned);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(generation: u64, ticks: u64, total_us: u64) -> Sample {
        Sample {
            generation,
            entered: ticks,
            returned: ticks + 2,
            total_us,
            errors: 0,
            valid: true,
        }
    }

    #[test]
    fn cumulative_drains_preserve_intermediate_reads_and_reject_generation_drift() {
        let mut session = Session::new();
        let start = session.request(3, 8, true, false).unwrap();
        session.store(start, Role::Root, sample(1, 100, 1000));
        let end = session.request(3, 8, true, true).unwrap();
        session.store(end, Role::Root, sample(1, 500, 1240));
        assert_eq!(session.delta(Role::Root), Some(240));
        session.end[0].as_mut().unwrap().generation = 2;
        assert_eq!(session.delta(Role::Root), None);
    }

    #[test]
    fn asynchronous_late_begin_cannot_complete_an_end_or_replacement_session() {
        let mut session = Session::new();
        let start = session.request(1, 1, false, false).unwrap();
        assert_eq!(session.selected, 0b111111011);
        session.request(1, 1, false, true).unwrap();
        session.store(start, Role::Cyw43, sample(1, 10, 0));
        assert_eq!(session.begin[3], None);
        assert_eq!(session.pending, 0b111111011);
        let replacement = session.request(1, 2, true, false).unwrap();
        assert_ne!(start, replacement);
        session.store(start, Role::Root, sample(1, 20, 10));
        assert_eq!(session.begin[0], None);
        assert_eq!(session.selected, 0b111100111);
        assert!(session.request(1, 1, false, true).is_none());
    }

    #[test]
    fn missing_error_and_reversed_samples_never_report_cpu_zero_as_valid() {
        let mut session = Session::new();
        let start = session.request(1, 1, true, false).unwrap();
        session.store(start, Role::Genet, sample(4, 100, 50));
        let end = session.request(1, 1, true, true).unwrap();
        assert_eq!(session.delta(Role::Genet), None);
        let mut failed = sample(4, 200, 60);
        failed.errors = 1;
        session.store(end, Role::Genet, failed);
        assert_eq!(session.delta(Role::Genet), None);
        session.end[2] = Some(sample(4, 90, 60));
        assert_eq!(session.delta(Role::Genet), None);
        session.end[2] = Some(sample(4, 200, 40));
        assert_eq!(session.delta(Role::Genet), None);
    }

    #[test]
    fn maximum_width_rows_preserve_validity_and_all_counter_endpoints() {
        let mut session = Session::new();
        session.generation = u64::MAX;
        session.connection = u64::MAX;
        session.selected = 0b111111111;
        session.finish = true;
        for index in 0..ROLES {
            session.begin[index] = Some(Sample {
                generation: u64::MAX,
                entered: u64::MAX - 3,
                returned: u64::MAX - 2,
                total_us: 0,
                errors: 0,
                valid: true,
            });
            session.end[index] = Some(Sample {
                generation: u64::MAX,
                entered: u64::MAX - 1,
                returned: u64::MAX,
                total_us: u64::MAX,
                errors: 0,
                valid: true,
            });
        }
        let lines = render(&session, 54_000_000);
        assert!(lines[0].ends_with("hz=54000000"));
        for line in &lines[1..] {
            assert!(line.contains("valid=true cpu_us=18446744073709551615"));
            assert!(line.ends_with("end=fffffffffffffffe/ffffffffffffffff"));
            assert!(line.len() <= DEFAULT_LINE_CAPACITY);
        }
    }

    #[test]
    fn each_driver_is_claimed_once_even_when_its_result_cannot_be_stored() {
        let mut session = Session::new();
        let begin = session.request(1, 1, false, false).unwrap();
        assert_eq!(session.claim_driver(), Some((begin, Role::Cyw43)));
        assert_eq!(session.claim_driver(), Some((begin, Role::Sdio)));
        for role in [Role::Serial, Role::Usb, Role::Hdmi, Role::Pcie] {
            assert_eq!(session.claim_driver(), Some((begin, role)));
        }
        assert_eq!(session.claim_driver(), None);
        assert_eq!(session.claimed, 0b111111000);
        assert_eq!(session.pending, 0b00011);
        let end = session.request(1, 1, false, true).unwrap();
        assert_eq!(session.claim_driver(), Some((end, Role::Cyw43)));
        session.store(begin, Role::Cyw43, sample(1, 100, 20));
        assert_eq!(session.begin[3], None);
        assert_eq!(session.delta(Role::Cyw43), None);
    }
}
