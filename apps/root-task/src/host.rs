// Author: Lukas Bower
#![allow(clippy::module_name_repetitions)]

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result as AnyhowResult};
use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{FrameHeader, SessionId};

use crate::console::{Command, CommandParser, ConsoleError};
#[cfg(feature = "net")]
use crate::net::NetStack;
#[cfg(feature = "net")]
use smoltcp::wire::Ipv4Address;

/// Result alias used throughout the host-mode simulation.
pub type Result<T> = AnyhowResult<T>;

/// Entry point for host-mode execution of the root task simulation.
pub fn main() -> Result<()> {
    let stdout = io::stdout();
    let handle = stdout.lock();
    let timer = SleepTimer::new(Duration::from_millis(25), TICK_LIMIT);
    let component = PingPongComponent::new();
    let mut root_task = RootTask::new(handle, timer, component)?;
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
    console: CommandParser,
    #[cfg(feature = "net")]
    net: NetStack,
}

impl<W: Write, T: Timer, C: UserComponent> RootTask<W, T, C> {
    /// Create a new root task simulation.
    fn new(writer: W, timer: T, component: C) -> Result<Self> {
        #[cfg(feature = "net")]
        let (net, _) = NetStack::new(Ipv4Address::new(10, 0, 0, 2));

        Ok(Self {
            writer,
            timer,
            component,
            ping_sent: false,
            bootstrap_ticket: TicketTemplate::new(Role::Queen, BudgetSpec::unbounded()),
            console: CommandParser::new(),
            #[cfg(feature = "net")]
            net,
        })
    }

    fn run(&mut self) -> Result<()> {
        self.log_banner()?;
        let handle = self.spawn_component()?;
        self.log_component_spawn(&handle)?;
        self.seed_console()?;
        while let Some(tick) = self.timer.next_tick() {
            self.log_tick(tick.count)?;
            if !self.ping_sent {
                self.perform_ping_pong(tick.count)?;
            }
            #[cfg(feature = "net")]
            self.poll_network()?;
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

    fn seed_console(&mut self) -> Result<()> {
        if let Err(err) = self.console.record_login_attempt(false, 1_000) {
            writeln!(self.writer, "login limiter warning: {err}")?;
        }
        if let Err(err) = self.console.record_login_attempt(false, 2_000) {
            writeln!(self.writer, "login limiter warning: {err}")?;
        }
        if let Err(err) = self.console.record_login_attempt(false, 3_000) {
            writeln!(self.writer, "login limiter warning: {err}")?;
        }
        self.console
            .record_login_attempt(true, 120_000)
            .map_err(|err| anyhow!(err.to_string()))?;

        const SCRIPT: &[&str] = &["help", "attach queen", "log", "tail /log/queen.log", "quit"];
        for line in SCRIPT {
            for byte in line.as_bytes() {
                if let Some(command) = self.console.push_byte(*byte).map_err(to_anyhow)? {
                    self.handle_command(command)?;
                }
            }
            if let Some(command) = self.console.push_byte(b'\n').map_err(to_anyhow)? {
                self.handle_command(command)?;
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Help => writeln!(self.writer, "console: help requested")?,
            Command::Attach { role, ticket } => {
                writeln!(
                    self.writer,
                    "console: attach role={} ticket={}",
                    role,
                    ticket.as_deref().unwrap_or("<none>")
                )?;
            }
            Command::Tail { path } => {
                writeln!(self.writer, "console: tail {}", path)?;
            }
            Command::Log => writeln!(self.writer, "console: log streaming enabled")?,
            Command::Quit => writeln!(self.writer, "console: quit requested")?,
            Command::Spawn(payload) => {
                writeln!(self.writer, "console: spawn {}", payload)?;
            }
            Command::Kill(ident) => {
                writeln!(self.writer, "console: kill {}", ident)?;
            }
        }
        Ok(())
    }

    #[cfg(feature = "net")]
    fn poll_network(&mut self) -> Result<()> {
        use crate::net::Frame;
        let handle = self.net.queue_handle();
        if handle.pop_tx().is_none() {
            let frame = Frame::from_slice(&[0u8; 64]).map_err(|err| anyhow!(err.to_string()))?;
            handle
                .push_rx(frame)
                .map_err(|err| anyhow!(err.to_string()))?;
        }
        let changed = self.net.poll(10);
        if changed {
            writeln!(
                self.writer,
                "net: polled interface {:?}",
                self.net.hardware_address()
            )?;
        }
        Ok(())
    }
}

fn to_anyhow(err: ConsoleError) -> anyhow::Error {
    anyhow!(err.to_string())
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
        let mut root_task = RootTask::new(&mut output, timer, component).unwrap();
        root_task.run().unwrap();
        let transcript = String::from_utf8(output).unwrap();
        assert!(transcript.contains("PING 1"));
        assert!(transcript.contains("PONG 1"));
        assert_eq!(transcript.matches("PING").count(), 1);
    }
}
