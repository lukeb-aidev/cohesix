// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Author: Lukas Bower
// Purpose: Accumulate bounded passive timing evidence for isolated network-service seams.

use core::fmt::Write;

use heapless::String;

use crate::serial::DEFAULT_LINE_CAPACITY;

/// Convert one absolute architected-counter sample to the child service's
/// millisecond epoch.
///
/// Keeping the conversion here makes the target-only sample path directly
/// comparable with `console-network-runtime` while leaving ordinary root
/// timer and smoltcp time domains untouched.
const fn architected_counter_ms(counter: u64, timer_clock_hz: u64) -> u64 {
    if timer_clock_hz == 0 {
        return 0;
    }
    let seconds = counter / timer_clock_hz;
    let remainder = counter % timer_clock_hz;
    seconds
        .saturating_mul(1_000)
        .saturating_add(remainder.saturating_mul(1_000) / timer_clock_hz)
}

/// Sample the common child/root seam epoch on a physical Pi release target.
///
/// The isolated child publishes absolute `CNTVCT_EL0` time. Only the physical
/// Pi release target replaces the caller's ordinary elapsed-time fallback with
/// that absolute epoch. Host tests and every QEMU build retain the fallback,
/// so this helper cannot alter runtime or smoltcp scheduling time.
#[inline]
pub(crate) fn isolated_seam_observation_ms(_fallback_ms: u64) -> u64 {
    #[cfg(all(feature = "release-pi4", target_arch = "aarch64", target_os = "none"))]
    {
        return architected_counter_ms(
            crate::arch::aarch64::timer::timer_counter_ticks(),
            crate::arch::aarch64::timer::timer_freq_hz(),
        );
    }

    #[cfg(not(all(feature = "release-pi4", target_arch = "aarch64", target_os = "none")))]
    {
        _fallback_ms
    }
}

/// One passive age accumulator for a causally matched isolated-service seam.
///
/// The selected exchange ABI already carries millisecond publication time.
/// This accumulator observes that immutable evidence after ordinary validation;
/// it cannot authorize a wake, retry, continuation, or scheduling decision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IsolatedSeamAgeDiagnostic {
    /// Valid monotonic age samples.
    pub samples: u64,
    /// Saturating sum used to derive the bounded average.
    pub total_ms: u64,
    /// Most recently accepted age.
    pub last_ms: u64,
    /// Largest accepted age.
    pub max_ms: u64,
    /// Zero or backward timestamp pairs rejected from the latency aggregate.
    pub invalid_samples: u64,
    /// Millisecond age bins: [0,1), [1,2), [2,4), [4,8), and [8,infinity).
    pub histogram: [u64; 5],
}

impl IsolatedSeamAgeDiagnostic {
    pub(crate) fn record(&mut self, published_ms: u64, observed_ms: u64) {
        let Some(age_ms) = observed_ms
            .checked_sub(published_ms)
            .filter(|_| published_ms != 0 && observed_ms != 0)
        else {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        };
        self.samples = self.samples.saturating_add(1);
        self.total_ms = self.total_ms.saturating_add(age_ms);
        self.last_ms = age_ms;
        self.max_ms = self.max_ms.max(age_ms);
        let bin = match age_ms {
            0 => 0,
            1 => 1,
            2..=3 => 2,
            4..=7 => 3,
            _ => 4,
        };
        self.histogram[bin] = self.histogram[bin].saturating_add(1);
    }
}

/// Passive phase splits for the copied-WiFi and direct-GENET console service.
///
/// These aggregates distinguish child queueing and child-to-root delay from
/// root-to-child control delay without adding a shared ABI field or a routine
/// serial log. The module is target-Pi-only outside tests so the protected QEMU
/// release path does not execute the accounting writes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IsolatedSeamDiagnostics {
    /// Child command creation to validated root observation, including queueing.
    pub command_create_to_root_observe: IsolatedSeamAgeDiagnostic,
    /// First batched command creation to the child's publication-start sample.
    pub command_create_to_publish: IsolatedSeamAgeDiagnostic,
    /// Child publication-start sample to validated root observation.
    pub command_publish_to_root_observe: IsolatedSeamAgeDiagnostic,
    /// Complete copied command plus egress pairs accepted under one credit.
    pub paired_publications: u64,
    /// Root command dispatch to the first durable response StageOutput.
    pub dispatch_to_stage: IsolatedSeamAgeDiagnostic,
    /// Root StageOutput publication to observed control-consumption watermark.
    pub stage_to_control_observe: IsolatedSeamAgeDiagnostic,
    /// Root StageOutput publication to child OutputDrained publication.
    pub stage_to_output_drained: IsolatedSeamAgeDiagnostic,
    /// Child OutputDrained publication to validated root observation.
    pub output_drained_publish_to_root_observe: IsolatedSeamAgeDiagnostic,
}

impl IsolatedSeamDiagnostics {
    /// Seven complete bounded rows; histogram display clips at 65535 and
    /// reports clipping explicitly while internal counters retain full width.
    pub(crate) fn lines(&self) -> [String<DEFAULT_LINE_CAPACITY>; 7] {
        let entries = [
            (
                "command-created-root-observe",
                self.command_create_to_root_observe,
            ),
            ("command-created-publish", self.command_create_to_publish),
            (
                "command-publish-root-observe",
                self.command_publish_to_root_observe,
            ),
            ("dispatch-stage", self.dispatch_to_stage),
            ("stage-control-observe", self.stage_to_control_observe),
            ("stage-output-drained", self.stage_to_output_drained),
            (
                "output-drained-root-observe",
                self.output_drained_publish_to_root_observe,
            ),
        ];
        core::array::from_fn(|index| {
            let (name, age) = entries[index];
            let bins = age.histogram.map(|n| n.min(u64::from(u16::MAX)));
            let mut line = String::new();
            let _ = write!(line,
                "netstats: isolated_seam schema=v2 name={} n={} bad={} ms={}/{}/{} h={:x}/{:x}/{:x}/{:x}/{:x} hs={}",
                name, age.samples, age.invalid_samples, age.total_ms, age.last_ms, age.max_ms,
                bins[0], bins[1], bins[2], bins[3], bins[4],
                u8::from(age.histogram.iter().any(|n| *n > u64::from(u16::MAX))),
            );
            if index == 1 {
                let _ = write!(line, " pairs={}", self.paired_publications);
            }
            line
        })
    }

    pub(crate) fn record_command_or_batch_observed(&mut self, created_ms: u64, observed_ms: u64) {
        self.command_create_to_root_observe
            .record(created_ms, observed_ms);
    }

    pub(crate) fn record_command_publication(
        &mut self,
        created_ms: u64,
        published_ms: u64,
        observed_ms: u64,
    ) {
        self.record_command_or_batch_observed(created_ms, observed_ms);
        self.command_create_to_publish
            .record(created_ms, published_ms);
        self.command_publish_to_root_observe
            .record(published_ms, observed_ms);
    }

    pub(crate) fn record_dispatch_to_stage(&mut self, dispatch_ms: u64, staged_ms: u64) {
        self.dispatch_to_stage.record(dispatch_ms, staged_ms);
    }

    pub(crate) fn record_control_completed(&mut self, staged_ms: u64, observed_ms: u64) {
        self.stage_to_control_observe.record(staged_ms, observed_ms);
    }

    pub(crate) fn record_output_drained(
        &mut self,
        staged_ms: u64,
        published_ms: u64,
        observed_ms: u64,
    ) {
        self.stage_to_output_drained.record(staged_ms, published_ms);
        self.output_drained_publish_to_root_observe
            .record(published_ms, observed_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_split_and_histogram_preserve_distinct_intervals() {
        let mut diagnostics = IsolatedSeamDiagnostics::default();
        diagnostics.record_command_publication(100, 111, 115);
        assert_eq!(diagnostics.command_create_to_root_observe.total_ms, 15);
        assert_eq!(diagnostics.command_create_to_publish.total_ms, 11);
        assert_eq!(diagnostics.command_publish_to_root_observe.total_ms, 4);
        assert_eq!(
            diagnostics.command_publish_to_root_observe.histogram,
            [0, 0, 0, 1, 0]
        );
        let mut age = IsolatedSeamAgeDiagnostic::default();
        for delay in [0, 1, 2, 3, 4, 7, 8, 19] {
            age.record(100, 100 + delay);
        }
        age.record(0, 20);
        age.record(30, 29);
        assert_eq!(age.histogram, [1, 1, 2, 2, 2]);
        assert_eq!((age.samples, age.invalid_samples), (8, 2));
    }

    #[test]
    fn complete_seam_rows_fit_existing_console_bound_and_report_histogram_clipping() {
        let age = IsolatedSeamAgeDiagnostic {
            samples: u64::MAX,
            invalid_samples: u64::MAX,
            total_ms: u64::MAX,
            last_ms: u64::MAX,
            max_ms: u64::MAX,
            histogram: [u64::MAX; 5],
        };
        let diagnostics = IsolatedSeamDiagnostics {
            command_create_to_root_observe: age,
            command_create_to_publish: age,
            command_publish_to_root_observe: age,
            dispatch_to_stage: age,
            stage_to_control_observe: age,
            stage_to_output_drained: age,
            output_drained_publish_to_root_observe: age,
            paired_publications: u64::MAX,
        };
        for (index, line) in diagnostics.lines().iter().enumerate() {
            assert!(line.len() < 256);
            assert!(line.contains("h=ffff/ffff/ffff/ffff/ffff hs=1"));
            if index == 1 {
                assert!(line.ends_with("pairs=18446744073709551615"));
            } else {
                assert!(line.ends_with("hs=1"));
            }
        }
        assert_eq!(IsolatedSeamDiagnostics::default().lines()[3].as_str(),
            "netstats: isolated_seam schema=v2 name=dispatch-stage n=0 bad=0 ms=0/0/0 h=0/0/0/0/0 hs=0");
    }

    #[test]
    fn architected_counter_conversion_matches_child_epoch() {
        assert_eq!(architected_counter_ms(0, 54_000_000), 0);
        assert_eq!(architected_counter_ms(81_000_000, 54_000_000), 1_500);
        assert_eq!(architected_counter_ms(u64::MAX, 0), 0);
    }

    #[cfg(not(all(feature = "release-pi4", target_arch = "aarch64", target_os = "none")))]
    #[test]
    fn non_pi_target_preserves_caller_time_domain() {
        assert_eq!(isolated_seam_observation_ms(12_345), 12_345);
    }

    #[test]
    fn age_accepts_monotonic_samples_and_saturates() {
        let mut diagnostic = IsolatedSeamAgeDiagnostic::default();
        diagnostic.record(100, 111);
        diagnostic.record(200, 209);
        assert_eq!(diagnostic.samples, 2);
        assert_eq!(diagnostic.total_ms, 20);
        assert_eq!(diagnostic.last_ms, 9);
        assert_eq!(diagnostic.max_ms, 11);
        assert_eq!(diagnostic.invalid_samples, 0);

        diagnostic.samples = u64::MAX;
        diagnostic.total_ms = u64::MAX;
        diagnostic.record(300, 307);
        assert_eq!(diagnostic.samples, u64::MAX);
        assert_eq!(diagnostic.total_ms, u64::MAX);
        assert_eq!(diagnostic.last_ms, 7);
        assert_eq!(diagnostic.max_ms, 11);
    }

    #[test]
    fn age_rejects_zero_and_backward_pairs_without_pollution() {
        let mut diagnostic = IsolatedSeamAgeDiagnostic::default();
        for (published_ms, observed_ms) in [(0, 10), (10, 0), (11, 10)] {
            diagnostic.record(published_ms, observed_ms);
        }
        assert_eq!(diagnostic.samples, 0);
        assert_eq!(diagnostic.total_ms, 0);
        assert_eq!(diagnostic.last_ms, 0);
        assert_eq!(diagnostic.max_ms, 0);
        assert_eq!(diagnostic.invalid_samples, 3);

        diagnostic.invalid_samples = u64::MAX;
        diagnostic.record(20, 19);
        assert_eq!(diagnostic.invalid_samples, u64::MAX);
    }

    #[test]
    fn routes_command_control_and_drain_ages_independently() {
        let mut diagnostics = IsolatedSeamDiagnostics::default();

        // One Command and one CommandBatch publication observed by root.
        diagnostics.record_command_or_batch_observed(100, 107);
        diagnostics.record_command_or_batch_observed(200, 211);
        // The same staged response is observed on the control and event paths.
        diagnostics.record_dispatch_to_stage(290, 300);
        diagnostics.record_control_completed(300, 305);
        diagnostics.record_output_drained(300, 308, 313);

        assert_eq!(
            diagnostics.command_create_to_root_observe,
            IsolatedSeamAgeDiagnostic {
                samples: 2,
                total_ms: 18,
                last_ms: 11,
                max_ms: 11,
                invalid_samples: 0,
                histogram: [0, 0, 0, 1, 1],
            }
        );
        assert_eq!(diagnostics.dispatch_to_stage.last_ms, 10);
        assert_eq!(diagnostics.stage_to_control_observe.last_ms, 5);
        assert_eq!(diagnostics.stage_to_output_drained.last_ms, 8);
        assert_eq!(
            diagnostics.output_drained_publish_to_root_observe.last_ms,
            5
        );

        // A backward child publication invalidates only the stage-to-child
        // edge; its later child-to-root observation remains independently
        // useful and must not contaminate the valid latency aggregate.
        diagnostics.record_output_drained(400, 399, 402);
        assert_eq!(diagnostics.stage_to_output_drained.samples, 1);
        assert_eq!(diagnostics.stage_to_output_drained.invalid_samples, 1);
        assert_eq!(
            diagnostics.output_drained_publish_to_root_observe.samples,
            2
        );
        assert_eq!(
            diagnostics.output_drained_publish_to_root_observe.last_ms,
            3
        );
    }
}
