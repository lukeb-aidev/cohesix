// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-08

//! Role module for the Cohesix `Worker`.
//! A worker node executes assigned tasks, reports telemetry, and responds to commands from the queen or orchestrator.

/// Trait representing worker responsibilities.
pub trait WorkerRole {
    fn execute_task(&mut self, task: &str) -> Result<(), String>;
    fn report_telemetry(&self) -> String;
    fn receive_command(&mut self, cmd: &str) -> Result<(), String>;
}

use sysinfo::{System, SystemExt};

/// Basic worker implementation.
pub struct DefaultWorker;

impl WorkerRole for DefaultWorker {
    fn execute_task(&mut self, task: &str) -> Result<(), String> {
        println!("[worker] executing task '{}'", task);
        if let Some(expr) = task.strip_prefix("compute ") {
            match crate::utils::const_eval::eval(expr) {
                Ok(v) => {
                    println!("[worker] result: {}", v);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            println!("[worker] no-op task");
            Ok(())
        }
    }

    fn report_telemetry(&self) -> String {
        println!("[worker] reporting telemetry...");
        let mut sys = System::new();
        sys.refresh_system();
        let mem = sys.used_memory();
        let total_mem = sys.total_memory();
        format!("mem:{}/{}kb", mem, total_mem)
    }

    fn receive_command(&mut self, cmd: &str) -> Result<(), String> {
        println!("[worker] received command '{}'", cmd);
        if let Some(task) = cmd.strip_prefix("task:") {
            self.execute_task(task.trim())
        } else if cmd == "report" {
            let t = self.report_telemetry();
            println!("[worker] telemetry: {}", t);
            Ok(())
        } else {
            Err(format!("unknown command '{}'", cmd))
        }
    }
}
