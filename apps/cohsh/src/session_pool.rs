// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide bounded session pooling for cohsh transports.
// Author: Lukas Bower

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use cohesix_ticket::Role;

use crate::{Session, Transport};

/// Distinguish pooled sessions used for control vs telemetry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolKind {
    /// Pool used for control-plane operations.
    Control,
    /// Pool used for telemetry and bulk operations.
    Telemetry,
}

/// Transport factory used to create pooled sessions.
pub trait TransportFactory: Send + Sync {
    /// Construct a new transport instance for the pool.
    fn create(&self) -> Result<Box<dyn Transport + Send>>;
}

impl<F> TransportFactory for F
where
    F: Fn() -> Result<Box<dyn Transport + Send>> + Send + Sync,
{
    fn create(&self) -> Result<Box<dyn Transport + Send>> {
        (self)()
    }
}

struct PoolSession {
    transport: Box<dyn Transport + Send>,
    session: Session,
}

#[derive(Default)]
struct PoolState {
    role: Option<Role>,
    ticket: Option<String>,
    closed: bool,
    control_total: u16,
    telemetry_total: u16,
    control_idle: VecDeque<PoolSession>,
    telemetry_idle: VecDeque<PoolSession>,
}

/// Session pool sized by manifest policy.
#[derive(Clone)]
pub struct SessionPool {
    control_capacity: u16,
    telemetry_capacity: u16,
    factory: Arc<dyn TransportFactory>,
    state: Arc<Mutex<PoolState>>,
}

impl SessionPool {
    /// Create a new pool with the specified capacities.
    pub fn new(
        control_capacity: u16,
        telemetry_capacity: u16,
        factory: Arc<dyn TransportFactory>,
    ) -> Self {
        Self {
            control_capacity: control_capacity.max(1),
            telemetry_capacity: telemetry_capacity.max(1),
            factory,
            state: Arc::new(Mutex::new(PoolState::default())),
        }
    }

    /// Configure the pool for a new role and ticket, warming the pool to capacity.
    pub fn attach(&self, role: Role, ticket: Option<&str>) -> Result<()> {
        let mut state = self.state.lock().expect("session pool lock poisoned");
        self.reset_locked(&mut state);
        state.role = Some(role);
        state.ticket = ticket
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        state.closed = false;

        let extra_control = self.control_capacity.saturating_sub(1);
        for _ in 0..extra_control {
            let session = self.spawn_session(role, state.ticket.as_deref())?;
            state.control_idle.push_back(session);
            state.control_total = state.control_total.saturating_add(1);
        }
        for _ in 0..self.telemetry_capacity {
            let session = self.spawn_session(role, state.ticket.as_deref())?;
            state.telemetry_idle.push_back(session);
            state.telemetry_total = state.telemetry_total.saturating_add(1);
        }
        Ok(())
    }

    /// Shut down and clear pooled sessions.
    pub fn shutdown(&self) {
        let mut state = self.state.lock().expect("session pool lock poisoned");
        state.closed = true;
        self.reset_locked(&mut state);
    }

    /// Return the configured pool capacities.
    pub fn capacities(&self) -> (u16, u16) {
        (self.control_capacity, self.telemetry_capacity)
    }

    /// Borrow a session from the pool for the requested kind.
    pub fn checkout(&self, kind: PoolKind) -> Result<PoolLease> {
        let mut state = self.state.lock().expect("session pool lock poisoned");
        if state.closed {
            return Err(anyhow!("session pool is closed"));
        }
        let role = state
            .role
            .ok_or_else(|| anyhow!("session pool is not attached"))?;
        let ticket = state.ticket.clone();
        match kind {
            PoolKind::Control => {
                if let Some(session) = state.control_idle.pop_front() {
                    return Ok(PoolLease::new(kind, self.state.clone(), session));
                }
                if state.control_total >= self.control_capacity {
                    return Err(anyhow!("session pool exhausted for {kind:?}"));
                }
                let session = self.spawn_session(role, ticket.as_deref())?;
                state.control_total = state.control_total.saturating_add(1);
                Ok(PoolLease::new(kind, self.state.clone(), session))
            }
            PoolKind::Telemetry => {
                if let Some(session) = state.telemetry_idle.pop_front() {
                    return Ok(PoolLease::new(kind, self.state.clone(), session));
                }
                if state.telemetry_total >= self.telemetry_capacity {
                    return Err(anyhow!("session pool exhausted for {kind:?}"));
                }
                let session = self.spawn_session(role, ticket.as_deref())?;
                state.telemetry_total = state.telemetry_total.saturating_add(1);
                Ok(PoolLease::new(kind, self.state.clone(), session))
            }
        }
    }

    fn spawn_session(&self, role: Role, ticket: Option<&str>) -> Result<PoolSession> {
        let mut transport = self.factory.create()?;
        let session = transport.attach(role, ticket)?;
        let _ = transport.drain_acknowledgements();
        Ok(PoolSession { transport, session })
    }

    fn reset_locked(&self, state: &mut PoolState) {
        for mut session in state.control_idle.drain(..) {
            let _ = session.transport.quit(&session.session);
        }
        for mut session in state.telemetry_idle.drain(..) {
            let _ = session.transport.quit(&session.session);
        }
        state.control_total = 0;
        state.telemetry_total = 0;
        state.role = None;
        state.ticket = None;
    }
}

/// A pooled session lease returned to the pool when dropped.
pub struct PoolLease {
    kind: PoolKind,
    state: Arc<Mutex<PoolState>>,
    session: Option<PoolSession>,
}

impl PoolLease {
    fn new(kind: PoolKind, state: Arc<Mutex<PoolState>>, session: PoolSession) -> Self {
        Self {
            kind,
            state,
            session: Some(session),
        }
    }

    /// Return the session metadata for this lease.
    pub fn session(&self) -> &Session {
        &self
            .session
            .as_ref()
            .expect("pool lease missing session")
            .session
    }

    /// Return a mutable reference to the underlying transport.
    pub fn transport_mut(&mut self) -> &mut dyn Transport {
        self.session
            .as_mut()
            .expect("pool lease missing session")
            .transport
            .as_mut()
    }
}

impl Drop for PoolLease {
    fn drop(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        let mut state = self.state.lock().expect("session pool lock poisoned");
        if state.closed {
            let _ = session.transport.quit(&session.session);
            return;
        }
        match self.kind {
            PoolKind::Control => state.control_idle.push_back(session),
            PoolKind::Telemetry => state.telemetry_idle.push_back(session),
        }
    }
}
