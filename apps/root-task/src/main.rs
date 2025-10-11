// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Root task bootstrap logic per `docs/ARCHITECTURE.md` §1-§3.
//!
//! The Cohesix root task is responsible for taking ownership of the initial
//! capabilities provided by seL4, configuring timers, and orchestrating
//! subordinate services such as the NineDoor 9P server and worker suites. This
//! placeholder binary documents the responsibilities while the seL4 bindings
//! and runtime scaffolding are prepared in later milestones.

use anyhow::Result;
use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::FrameHeader;

fn main() -> Result<()> {
    let ticket = TicketTemplate::new(Role::Queen, BudgetSpec::unbounded());
    let frame = FrameHeader::new(0, 0);
    println!(
        "Cohesix root-task stub — queen ticket {:?}, frame {:?}",
        ticket.role(),
        frame
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_stub_runs() {
        main().expect("stub execution should succeed");
    }
}
