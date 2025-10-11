// Author: Lukas Bower
#![allow(clippy::module_name_repetitions)]

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result as AnyhowResult};
use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{FrameHeader, SessionId};

/// Result alias used throughout the host-mode simulation.
pub type Result<T> = AnyhowResult<T>;

/// Entry point for host-mode execution of the root task simulation.
pub fn main() -> Result<()> {
    let stdout = io::stdout();
    let handle = stdout.lock();
    let timer = SleepTimer::new(Duration::from_millis(25), TICK_LIMIT);
    let component = PingPongComponent::new();
    let mut root_task = RootTask::new(handle, timer, component);
    root_task.run()
}

/// Number of timer ticks emitted before the milestone simulation terminates.
const TICK_LIMIT: u64 = 3;

/// Representation of a periodic timer that blocks the current thread between ticks.
struct SleepTimer {
    period: Duration,
    limit: u64,
    emitted: u64,
}

impl SleepTimer {
    /// Create a new timer that yields `limit` ticks spaced `period` apart.
    fn new(period: Duration, limit: u64) -> Self {
        Self {
            period,
            limit,
            emitted: 0,
        }
    }
}

impl Timer for SleepTimer {
    fn next_tick(&mut self) -> Option<Tick> {
        if self.emitted >= self.limit {
            return None;
        }
        thread::sleep(self.period);
        self.emitted += 1;
        Some(Tick {
            count: self.emitted,
        })
    }
}

/// Abstraction over timer implementations so tests can operate deterministically.
trait Timer {
    /// Produce the next tick if the timer is still active.
    fn next_tick(&mut self) -> Option<Tick>;
}

/// Event emitted by timers to signal periodic work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tick {
    count: u64,
}

/// Basic simulation of a user component receiving pings from the root task.
#[derive(Debug, Default)]
struct PingPongComponent {
    spawned: bool,
    last_sequence: Option<u64>,
}

impl PingPongComponent {
    /// Construct a new component in the pre-spawn state.
    fn new() -> Self {
        Self::default()
    }
}

impl UserComponent for PingPongComponent {
    fn spawn(&mut self) -> Result<ComponentHandle> {
        if self.spawned {
            return Err(anyhow!("component already spawned"));
        }
        self.spawned = true;
        Ok(ComponentHandle {
            endpoint: FrameHeader::new(SessionId::from_raw(1), 0),
        })
    }

    fn ping(&mut self, sequence: u64) -> Result<u64> {
        if !self.spawned {
            return Err(anyhow!("component must be spawned before ping"));
        }
        self.last_sequence = Some(sequence);
        Ok(sequence)
    }
}

/// Minimal trait describing interactions with spawned user components.
trait UserComponent {
    /// Spawn the component and return a handle describing its endpoint.
    fn spawn(&mut self) -> Result<ComponentHandle>;

    /// Perform a ping/pong exchange using the provided sequence number.
    fn ping(&mut self, sequence: u64) -> Result<u64>;
}

/// Handle returned after a component spawn to document the endpoint handshake.
#[derive(Debug, Clone)]
struct ComponentHandle {
    endpoint: FrameHeader,
}

impl ComponentHandle {
    fn endpoint(&self) -> FrameHeader {
        self.endpoint
    }
}

/// Root task simulation that logs boot banners, timer ticks, and IPC handshakes.
struct RootTask<W: Write, T: Timer, C: UserComponent> {
    writer: W,
    timer: T,
    component: C,
    ping_sent: bool,
    bootstrap_ticket: TicketTemplate,
}

impl<W: Write, T: Timer, C: UserComponent> RootTask<W, T, C> {
    /// Create a new root task simulation.
    fn new(writer: W, timer: T, component: C) -> Self {
        Self {
            writer,
            timer,
            component,
            ping_sent: false,
            bootstrap_ticket: TicketTemplate::new(Role::Queen, BudgetSpec::unbounded()),
        }
    }

    fn run(&mut self) -> Result<()> {
        self.log_banner()?;
        let handle = self.spawn_component()?;
        self.log_component_spawn(&handle)?;
        while let Some(tick) = self.timer.next_tick() {
            self.log_tick(tick.count)?;
            if !self.ping_sent {
                self.perform_ping_pong(tick.count)?;
            }
        }
        writeln!(self.writer, "root-task shutdown")?;
        Ok(())
    }

    fn log_banner(&mut self) -> Result<()> {
        writeln!(
            self.writer,
            "Cohesix boot: root-task online (ticket role: {:?})",
            self.bootstrap_ticket.role()
        )?;
        Ok(())
    }

    fn spawn_component(&mut self) -> Result<ComponentHandle> {
        self.component.spawn()
    }

    fn log_component_spawn(&mut self, handle: &ComponentHandle) -> Result<()> {
        writeln!(
            self.writer,
            "spawned user-component endpoint {:?}",
            handle.endpoint()
        )?;
        Ok(())
    }

    fn log_tick(&mut self, tick: u64) -> Result<()> {
        writeln!(self.writer, "tick {}", tick)?;
        Ok(())
    }

    fn perform_ping_pong(&mut self, sequence: u64) -> Result<()> {
        writeln!(self.writer, "PING {}", sequence)?;
        let response = self.component.ping(sequence)?;
        writeln!(self.writer, "PONG {}", response)?;
        self.ping_sent = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTimer {
        ticks: core::iter::Take<core::ops::RangeInclusive<u64>>,
    }

    impl Timer for TestTimer {
        fn next_tick(&mut self) -> Option<Tick> {
            self.ticks.next().map(|count| Tick { count })
        }
    }

    #[test]
    fn ping_pong_only_runs_once() {
        let timer = TestTimer {
            ticks: (1..=5).take(3),
        };
        let component = PingPongComponent::new();
        let mut output = Vec::new();
        let mut root_task = RootTask::new(&mut output, timer, component);
        root_task.run().unwrap();
        let transcript = String::from_utf8(output).unwrap();
        assert!(transcript.contains("PING 1"));
        assert!(transcript.contains("PONG 1"));
        assert_eq!(transcript.matches("PING").count(), 1);
    }
}
