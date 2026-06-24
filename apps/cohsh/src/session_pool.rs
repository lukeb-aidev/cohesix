// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide bounded session pooling for cohsh transports.
// Author: Lukas Bower

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
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
            // Keep control-plane traffic sticky to the most recently used session.
            // Some control paths (for example policy approvals and gated writes)
            // rely on per-session sequencing semantics.
            PoolKind::Control => state.control_idle.push_front(session),
            PoolKind::Telemetry => state.telemetry_idle.push_back(session),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use anyhow::{Result, anyhow};
    use cohesix_ticket::Role;
    use secure9p_codec::SessionId;

    use super::{PoolKind, SessionPool, TransportFactory};
    use crate::{Session, Transport, TransportMetrics};

    #[derive(Debug)]
    struct TestTransport {
        next_session_id: std::sync::Arc<AtomicU64>,
    }

    impl Transport for TestTransport {
        fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<Session> {
            let id = self.next_session_id.fetch_add(1, Ordering::SeqCst);
            Ok(Session::new(SessionId::from_raw(id), role))
        }

        fn kind(&self) -> &'static str {
            "test"
        }

        fn ping(&mut self, _session: &Session) -> Result<String> {
            Ok("pong".to_owned())
        }

        fn tail(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
            Err(anyhow!("tail is not implemented"))
        }

        fn read(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
            Err(anyhow!("read is not implemented"))
        }

        fn list(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
            Err(anyhow!("list is not implemented"))
        }

        fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> Result<()> {
            Ok(())
        }

        fn metrics(&self) -> TransportMetrics {
            TransportMetrics::default()
        }
    }

    #[test]
    fn control_pool_reuses_most_recent_session() {
        let next_session_id = std::sync::Arc::new(AtomicU64::new(1));
        let factory: std::sync::Arc<dyn TransportFactory> = std::sync::Arc::new({
            let next_session_id = std::sync::Arc::clone(&next_session_id);
            move || {
                Ok(Box::new(TestTransport {
                    next_session_id: std::sync::Arc::clone(&next_session_id),
                }) as Box<dyn Transport + Send>)
            }
        });
        let pool = SessionPool::new(2, 2, factory);
        let attach_result = pool.attach(Role::Queen, None);
        assert!(
            attach_result.is_ok(),
            "attach pool failed: {:?}",
            attach_result.err()
        );
        if attach_result.is_err() {
            return;
        }

        let first_checkout = pool.checkout(PoolKind::Control);
        assert!(
            first_checkout.is_ok(),
            "first control checkout failed: {:?}",
            first_checkout.err()
        );
        let Some(first_lease) = first_checkout.ok() else {
            return;
        };
        let first_id = first_lease.session().id().into_raw();
        drop(first_lease);

        let second_checkout = pool.checkout(PoolKind::Control);
        assert!(
            second_checkout.is_ok(),
            "second control checkout failed: {:?}",
            second_checkout.err()
        );
        let Some(second_lease) = second_checkout.ok() else {
            return;
        };
        let second_id = second_lease.session().id().into_raw();
        drop(second_lease);
        assert_eq!(second_id, first_id);

        let checkout_a = pool.checkout(PoolKind::Control);
        assert!(
            checkout_a.is_ok(),
            "control checkout a failed: {:?}",
            checkout_a.err()
        );
        let Some(lease_a) = checkout_a.ok() else {
            return;
        };
        let checkout_b = pool.checkout(PoolKind::Control);
        assert!(
            checkout_b.is_ok(),
            "control checkout b failed: {:?}",
            checkout_b.err()
        );
        let Some(lease_b) = checkout_b.ok() else {
            return;
        };
        let id_a = lease_a.session().id().into_raw();
        let id_b = lease_b.session().id().into_raw();
        assert_ne!(id_a, id_b);
        drop(lease_a);
        drop(lease_b);

        let reused_checkout = pool.checkout(PoolKind::Control);
        assert!(
            reused_checkout.is_ok(),
            "reused control checkout failed: {:?}",
            reused_checkout.err()
        );
        let Some(reused_lease) = reused_checkout.ok() else {
            return;
        };
        let reused_id = reused_lease.session().id().into_raw();
        drop(reused_lease);
        assert_eq!(reused_id, id_b);
    }
}
