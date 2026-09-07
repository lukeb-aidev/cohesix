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

const RECEIVE_CPU_CAPACITY: usize = 128;

/// One asynchronous owner read. The receive counter cut is its identity;
/// neither this request nor the returned CPU value grants executable work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReceiveCpuRequest {
    epoch: u64,
    index: usize,
    finish: bool,
}

#[derive(Clone, Copy)]
struct ReceiveCpuRow {
    begin_ticks: u64,
    end_ticks: u64,
    begin: Option<Sample>,
    end: Option<Sample>,
    begin_claimed: bool,
    end_claimed: bool,
}

impl ReceiveCpuRow {
    const fn empty() -> Self {
        Self {
            begin_ticks: 0,
            end_ticks: 0,
            begin: None,
            end: None,
            begin_claimed: false,
            end_claimed: false,
        }
    }

    fn delta(self) -> Option<u64> {
        let begin = self.begin?;
        let end = self.end?;
        if !begin.valid
            || !end.valid
            || begin.generation != end.generation
            || begin.errors != end.errors
            || self.end_ticks == 0
            || begin.entered < self.begin_ticks
            || begin.returned > self.end_ticks
            || end.entered < self.end_ticks
            || end.entered < begin.returned
        {
            return None;
        }
        end.total_us.checked_sub(begin.total_us)
    }
}

struct ReceiveCpuJournal {
    epoch: u64,
    generation: u64,
    connection: u64,
    enabled: bool,
    attempts: u64,
    len: usize,
    rows: [ReceiveCpuRow; RECEIVE_CPU_CAPACITY],
}

impl ReceiveCpuJournal {
    const fn new() -> Self {
        Self {
            epoch: 0,
            generation: 0,
            connection: 0,
            enabled: false,
            attempts: 0,
            len: 0,
            rows: [ReceiveCpuRow::empty(); RECEIVE_CPU_CAPACITY],
        }
    }

    fn reset(&mut self, generation: u64, connection: u64, genet: bool) {
        self.enabled = false;
        let Some(epoch) = self.epoch.checked_add(1) else {
            return;
        };
        self.epoch = epoch;
        self.generation = generation;
        self.connection = connection;
        self.attempts = 0;
        self.len = 0;
        self.rows.fill(ReceiveCpuRow::empty());
        self.enabled = genet && generation != 0 && connection != 0;
    }

    fn matches(&self, generation: u64, connection: u64) -> bool {
        self.enabled && (self.generation, self.connection) == (generation, connection)
    }

    fn begin(&mut self, generation: u64, connection: u64, ticks: u64) -> bool {
        if !self.matches(generation, connection) || ticks == 0 {
            return false;
        }
        self.attempts = self.attempts.saturating_add(1);
        if self.len == RECEIVE_CPU_CAPACITY
            || self.len != 0 && self.rows[self.len - 1].begin_ticks >= ticks
        {
            return false;
        }
        self.rows[self.len].begin_ticks = ticks;
        self.len += 1;
        true
    }

    fn finish(&mut self, generation: u64, connection: u64, begin: u64, end: u64) -> bool {
        if !self.matches(generation, connection) || end < begin {
            return false;
        }
        let Some(row) = self.rows[..self.len]
            .iter_mut()
            .find(|row| row.begin_ticks == begin)
        else {
            return false;
        };
        if row.end_ticks != 0 {
            return false;
        }
        row.end_ticks = end;
        if !row.begin_claimed {
            // A receive that finished before its baseline was claimed cannot
            // yield an interval sample. Do not spend two late syscalls on it.
            row.begin_claimed = true;
            row.end_claimed = true;
            return false;
        }
        true
    }

    fn pending(&self) -> bool {
        self.enabled
            && self.rows[..self.len].iter().any(|row| {
                !row.begin_claimed || row.end_ticks != 0 && row.begin.is_some() && !row.end_claimed
            })
    }

    fn claim(&mut self) -> Option<ReceiveCpuRequest> {
        if !self.enabled {
            return None;
        }
        for (index, row) in self.rows[..self.len].iter_mut().enumerate() {
            let finish = if !row.begin_claimed {
                row.begin_claimed = true;
                false
            } else if row.end_ticks != 0 && row.begin.is_some() && !row.end_claimed {
                row.end_claimed = true;
                true
            } else {
                continue;
            };
            return Some(ReceiveCpuRequest {
                epoch: self.epoch,
                index,
                finish,
            });
        }
        None
    }

    fn store(&mut self, request: ReceiveCpuRequest, sample: Sample) {
        if request.epoch != self.epoch || request.index >= self.len || !self.enabled {
            return;
        }
        let row = &mut self.rows[request.index];
        let (claimed, target) = if request.finish {
            (row.end_claimed, &mut row.end)
        } else {
            (row.begin_claimed, &mut row.begin)
        };
        if claimed && target.is_none() {
            *target = Some(sample);
        }
    }
}

static RECEIVE_CPU: Mutex<ReceiveCpuJournal> = Mutex::new(ReceiveCpuJournal::new());

/// Request at most two GENET-owner reads for each of the first 128 receives.
/// This diagnostic never waits for its baseline or changes a receive route.
pub(crate) fn receive_begin(generation: u64, connection: u64, ticks: u64) -> bool {
    RECEIVE_CPU
        .try_lock()
        .is_some_and(|mut journal| journal.begin(generation, connection, ticks))
}

pub(crate) fn receive_finish(generation: u64, connection: u64, begin: u64, end: u64) -> bool {
    RECEIVE_CPU
        .try_lock()
        .is_some_and(|mut journal| journal.finish(generation, connection, begin, end))
}

pub(crate) fn receive_request() -> Option<ReceiveCpuRequest> {
    RECEIVE_CPU.try_lock()?.claim()
}

pub(crate) fn receive_pending() -> bool {
    RECEIVE_CPU
        .try_lock()
        .is_some_and(|journal| journal.pending())
}

pub(crate) fn store_receive(request: ReceiveCpuRequest, sample: Sample) {
    if let Some(mut journal) = RECEIVE_CPU.try_lock() {
        journal.store(request, sample);
    }
}

/// Append bounded provenance to the existing receive trace, without new rows.
/// `gc` is kernel CPU microseconds; `gb`/`ge` are the actual owner syscall
/// entry/return ticks. Their asynchronous margins remain observable.
pub(crate) fn append_receive_cpu(
    line: &mut String<DEFAULT_LINE_CAPACITY>,
    generation: u64,
    connection: u64,
    ticks: u64,
) {
    let Some(journal) = RECEIVE_CPU.try_lock() else {
        return;
    };
    if !journal.matches(generation, connection) {
        return;
    }
    let Some(row) = journal.rows[..journal.len]
        .iter()
        .find(|row| row.begin_ticks == ticks)
    else {
        let _ = line.push_str(" gc=na");
        return;
    };
    append_receive_row(line, *row);
}

fn append_receive_row(line: &mut String<DEFAULT_LINE_CAPACITY>, row: ReceiveCpuRow) {
    match row.delta() {
        Some(delta) => {
            let _ = write!(line, " gc={delta:x}");
        }
        None => {
            let _ = line.push_str(" gc=na");
        }
    }
    let begin = row.begin.unwrap_or_default();
    let end = row.end.unwrap_or_default();
    let _ = write!(
        line,
        " gb={:x}/{:x} ge={:x}/{:x}",
        begin.entered, begin.returned, end.entered, end.returned
    );
}

pub(crate) fn request(
    generation: u64,
    connection: u64,
    genet: bool,
    finish: bool,
) -> Option<Request> {
    let request = SESSION
        .try_lock()?
        .request(generation, connection, genet, finish)?;
    if !finish {
        if let Some(mut journal) = RECEIVE_CPU.try_lock() {
            journal.reset(generation, connection, genet);
        }
    }
    Some(request)
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

    #[test]
    fn receive_cpu_pairs_exact_owner_reads_and_preserves_async_margins() {
        let mut journal = ReceiveCpuJournal::new();
        journal.reset(7, 9, true);
        assert!(journal.begin(7, 9, 100));
        let begin = journal.claim().unwrap();
        assert!(!begin.finish);
        assert!(journal.claim().is_none());
        journal.store(begin, sample(3, 110, 500));
        assert!(!journal.pending());
        assert!(journal.finish(7, 9, 100, 200));
        let end = journal.claim().unwrap();
        assert!(end.finish);
        assert_eq!(begin.index, end.index);
        journal.store(end, sample(3, 210, 550));
        assert_eq!(journal.rows[0].delta(), Some(50));
        assert!(!journal.pending());
        let mut line = String::new();
        append_receive_row(&mut line, journal.rows[0]);
        assert_eq!(line, " gc=32 gb=6e/70 ge=d2/d4");
    }

    #[test]
    fn receive_cpu_early_completion_and_late_baselines_never_fabricate_zero() {
        let mut journal = ReceiveCpuJournal::new();
        journal.reset(1, 2, true);
        assert!(journal.begin(1, 2, 100));
        assert!(!journal.finish(1, 2, 100, 101));
        assert!(journal.claim().is_none());
        assert_eq!(journal.rows[0].delta(), None);

        assert!(journal.begin(1, 2, 200));
        let begin = journal.claim().unwrap();
        assert!(journal.finish(1, 2, 200, 210));
        journal.store(begin, sample(4, 211, 100));
        let end = journal.claim().unwrap();
        journal.store(end, sample(4, 220, 100));
        assert_eq!(journal.rows[1].delta(), None);

        let mut row = ReceiveCpuRow {
            begin_ticks: 100,
            end_ticks: 200,
            begin: Some(sample(4, 110, 100)),
            end: Some(sample(4, 210, 120)),
            ..ReceiveCpuRow::empty()
        };
        assert_eq!(row.delta(), Some(20));
        row.end.as_mut().unwrap().generation = 5;
        assert_eq!(row.delta(), None);
        row.end = Some(sample(4, 210, 99));
        assert_eq!(row.delta(), None);
        row.end = Some(sample(4, 210, 120));
        row.end.as_mut().unwrap().errors = 1;
        assert_eq!(row.delta(), None);
        row.end = Some(sample(4, 199, 120));
        assert_eq!(row.delta(), None);
        row.end = Some(sample(4, 210, 120));
        row.end.as_mut().unwrap().valid = false;
        assert_eq!(row.delta(), None);
    }

    #[test]
    fn receive_cpu_prefix_is_bounded_and_stale_results_cannot_cross_sessions() {
        let mut journal = ReceiveCpuJournal::new();
        journal.reset(1, 1, true);
        assert!(!journal.begin(1, 2, 1));
        assert!(!journal.begin(1, 1, 0));
        for index in 0..128 {
            let ticks = 100 + index * 10;
            assert!(journal.begin(1, 1, ticks));
            let begin = journal.claim().unwrap();
            journal.store(begin, sample(3, ticks + 1, index * 2));
            assert!(journal.finish(1, 1, ticks, ticks + 4));
            let end = journal.claim().unwrap();
            journal.store(end, sample(3, ticks + 5, index * 2 + 1));
        }
        assert_eq!(journal.len, 128);
        assert!(!journal.begin(1, 1, 2000));
        assert!(!journal.pending());
        assert!(journal.rows.iter().all(|row| row.delta() == Some(1)));

        journal.reset(1, 2, true);
        assert!(journal.begin(1, 2, 3000));
        let old = journal.claim().unwrap();
        journal.reset(1, 3, false);
        journal.store(old, sample(3, 3001, 400));
        assert!(!journal.begin(1, 3, 4000));
        assert!(journal.claim().is_none());
        journal.reset(1, 4, true);
        assert!(journal.begin(1, 4, 5000));
        journal.store(old, sample(3, 5001, 500));
        assert!(journal.rows[0].begin.is_none());
    }

    #[test]
    fn maximum_width_receive_cpu_suffix_retains_all_four_sample_ticks() {
        let mut line = String::new();
        write!(
            line,
            "[smp] receive n=15 outcome=unavailable cmd={:x} us={} ticks={:x}/{:x} hz={}",
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            u64::MAX
        )
        .unwrap();
        let row = ReceiveCpuRow {
            begin_ticks: 1,
            end_ticks: u64::MAX - 2,
            begin: Some(Sample {
                generation: u64::MAX,
                entered: u64::MAX - 4,
                returned: u64::MAX - 3,
                total_us: 0,
                valid: true,
                ..Sample::default()
            }),
            end: Some(Sample {
                generation: u64::MAX,
                entered: u64::MAX - 1,
                returned: u64::MAX,
                total_us: u64::MAX,
                valid: true,
                ..Sample::default()
            }),
            ..ReceiveCpuRow::empty()
        };
        append_receive_row(&mut line, row);
        assert!(line.contains(" gc=ffffffffffffffff"));
        assert!(line.ends_with("ge=fffffffffffffffe/ffffffffffffffff"));
        assert_eq!(line.len(), 241);
    }
}
