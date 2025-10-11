// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Root task bootstrap logic per `docs/ARCHITECTURE.md` §1-§3.
//!
//! Milestone 1 focuses on producing a deterministic boot banner, configuring a
//! periodic timer, and demonstrating a simple IPC handshake between the root
//! task and a spawned user component. The code below models these behaviours in
//! a host-friendly simulation so that the broader workspace can compile and the
//! CLI prototype can display meaningful output.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{FrameHeader, SessionId};

/// Number of timer ticks emitted before the milestone simulation terminates.
const TICK_LIMIT: u64 = 3;

fn main() -> Result<()> {
    let stdout = io::stdout();
    let handle = stdout.lock();
    let timer = SleepTimer::new(Duration::from_millis(25), TICK_LIMIT);
    let component = PingPongComponent::new();
    let mut root_task = RootTask::new(handle, timer, component);
    root_task.run()
}

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

/// Deterministic timer used during tests to avoid blocking sleeps.
#[cfg(test)]
struct MockTimer {
    events: Vec<Tick>,
}

#[cfg(test)]
impl MockTimer {
    fn new(events: Vec<Tick>) -> Self {
        Self { events }
    }
}

#[cfg(test)]
impl Timer for MockTimer {
    fn next_tick(&mut self) -> Option<Tick> {
        if self.events.is_empty() {
            None
        } else {
            Some(self.events.remove(0))
        }
    }
}

/// Test double for a component that records invocations.
#[cfg(test)]
#[derive(Default)]
struct MockComponent {
    spawned: bool,
    ping_calls: Vec<u64>,
}

#[cfg(test)]
impl UserComponent for MockComponent {
    fn spawn(&mut self) -> Result<ComponentHandle> {
        self.spawned = true;
        Ok(ComponentHandle {
            endpoint: FrameHeader::new(SessionId::from_raw(7), 0),
        })
    }

    fn ping(&mut self, sequence: u64) -> Result<u64> {
        if !self.spawned {
            return Err(anyhow!("component not spawned"));
        }
        self.ping_calls.push(sequence);
        Ok(sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_task_logs_boot_banner_and_ping_pong() {
        let timer = MockTimer::new(vec![Tick { count: 1 }, Tick { count: 2 }]);
        let component = MockComponent::default();
        let mut buffer = Vec::new();
        {
            let mut root = RootTask::new(&mut buffer, timer, component);
            root.run().expect("root task simulation should run");
        }
        let output = String::from_utf8(buffer).expect("valid UTF-8");
        assert!(output.contains("Cohesix boot: root-task online"));
        assert!(output.contains("tick 1"));
        assert!(output.contains("PING 1"));
        assert!(output.contains("PONG 1"));
        assert!(!output.matches("PING").skip(1).next().is_some());
    }
}
